
#![allow(dead_code, unused)]

use bytemuck::{Pod, Zeroable};
use cgmath::prelude::*;
use cgmath::{Matrix4, Vector3, Vector4};
use image::GenericImageView;
use std::borrow::Cow;
use std::mem;
use std::sync::Arc;
use tokio::runtime::Runtime;
use wgpu::util::DeviceExt;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::KeyCode;
use winit::window::{Window, WindowId};
use model::{DrawModel, Vertex};


mod camera;
mod camera_controller;
mod texture;
mod model;
mod resources;





struct Instances {
    position: cgmath::Vector3<f32>,
    rotation: cgmath::Quaternion<f32>,
}
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct InstanceRaw {
    model: [[f32; 4]; 4],
}

#[derive(Default)]
pub struct App<'a> {
    window: Option<Arc<Window>>,
    state: Option<GameState<'a>>,
}

struct GameState<'a> {
    surface: wgpu::Surface<'a>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,
    render_pipeline: wgpu::RenderPipeline,
    depth_texture: texture::Texture,
    camera: camera::Camera,
    camera_uniform: camera::CameraUniform,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    camera_controller: camera_controller::CameraController,
    instances: Vec<Instances>,
    instance_buffer: wgpu::Buffer,
    obj_model: model::Model,
}

impl Instances {
    fn to_raw(&self) -> InstanceRaw {
        InstanceRaw {
            model: (cgmath::Matrix4::from_translation(self.position)
                * cgmath::Matrix4::from(self.rotation))
            .into(),
        }
    }
}
impl<'a> GameState<'a> {
    async fn new(window: Arc<Window>) -> GameState<'a> {
        //define window size
        let size = window.inner_size();
        //create a WGPU instance
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        //use our instance to create a surface for wgpu to display to
        let surface = instance
            .create_surface(Arc::clone(&window))
            .expect("Failed to initialised surface");
        //create an adapter to the physical graphics device
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                ..Default::default()
            })
            .await
            .expect("Failed to get adapter");
//return the graphics device and command queue for the device. 
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await
            .expect("Failed to load device");
//returns the config for the adaptor in interact with the surface
        let config = surface
            .get_default_config(&adapter, size.width, size.height)
            .unwrap();
//initializes the surface for configuration
        surface.configure(&device, &config);


// This is to instancing of our object to display multiple copys of the same object, This will map
// 10 in x,y,z direction and rotate the object up to 45 degree as it gets further away
        let num_instances_per_row = 10;
        let instances = (0..num_instances_per_row)
            .flat_map(|z| {
                (0..num_instances_per_row).flat_map(move |x| {
                    (0..num_instances_per_row).map(move|y|{
                    let space_between = 3.0;
                    let position = cgmath::Vector3 {
                        x: space_between * (x as f32 - num_instances_per_row as f32 /2.0),
                        y: space_between * (y as f32 - num_instances_per_row as f32 /2.0),
                        z: space_between * (z as f32 - num_instances_per_row as f32 /2.0),
                    };

                    let rotation = if position.is_zero() {
                        cgmath::Quaternion::from_axis_angle(
                            cgmath::Vector3::unit_z(),
                            cgmath::Deg(0.0),
                        )
                    } else {
                        cgmath::Quaternion::from_axis_angle(position.normalize(), cgmath::Deg(45.0))
                    };

                    Instances { position, rotation }
                })
                }).collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
//takes our instance position and rotation to turn into a matrix4X4 so it can be read by the shader 
        let instance_data: Vec<InstanceRaw> = instances.iter().map(Instances::to_raw).collect();
        //puts the instance into the buffer
        let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Instance Buffer"),
            contents: bytemuck::cast_slice(&instance_data),
            usage: wgpu::BufferUsages::VERTEX,
        });
//define the layout of our bind group for our textures
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
                label: Some("texture_bind_group_layout"),
            });
//create our depth texture which will amend texel displayed based on depth rather than CW or CCW
let depth_texture = texture::Texture::create_depth_texture(&device, &config, "depth_texture");
//loading in our model and the associated texture
        let obj_model = resources::load_model("cube.obj", &device, &queue, &texture_bind_group_layout).await.unwrap();
//create our camera
        let camera_controller = camera_controller::CameraController::new();
        let mut camera = camera::Camera::new(size.width as f32, size.height as f32);
        let mut camera_uniform = camera::CameraUniform::new();
        //adds our camera into a buffer
        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
//define the layout of the camera bind group
        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: Some("camera_bind_group_layout"),
            });
        //create the camera bind group using our layout and load the buffer into it
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &camera_bind_group_layout,
            label: Some("camera_bind_group"),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });
    //define where the shader is and load it into the program
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

//define the render pipeline layout. which will need our bind group layouts that are needed to be
//rendered
        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[&texture_bind_group_layout, &camera_bind_group_layout],
                push_constant_ranges: &[],
            });
//create our render pipeline, and shaders attached to it. 
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main", // 1.
                buffers: &[
                    model::ModelVertex::desc(),
                    InstanceRaw::vertex_buffer_layout(),
                ], // 2.
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                // 3.
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    // 4.
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList, // 1.
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw, // 2.
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState{
                format: texture::Texture::DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }), // 1.
            multisample: wgpu::MultisampleState {
                count: 1,                       
                mask: !0,                        
                alpha_to_coverage_enabled: false,
            },
            multiview: None, 
        });

        Self {
            surface,
            device,
            queue,
            config,
            size,
            render_pipeline,
            depth_texture,
            camera,
            camera_uniform,
            camera_buffer,
            camera_bind_group,
            camera_controller,
            instances,
            instance_buffer,
            obj_model,
        }
    }
    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.size = new_size;
            self.camera.aspect = self.config.width as f32 / self.config.height as f32;
            self.surface.configure(&self.device, &self.config);
            self.depth_texture = texture::Texture::create_depth_texture(&self.device, &self.config, "depth_texture");
        }
    }
    fn input(&mut self, event: &WindowEvent) -> bool {
        self.camera_controller.process_events(event)
    }

    fn update(&mut self) {
        self.camera_controller.update_camera(&mut self.camera);
        self.camera_uniform.update_view_proj(&self.camera);
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::cast_slice(&[self.camera_uniform]),
        );
    }
    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture().ok().unwrap();
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[
                    // This is what @location(0) in the fragment shader targets
                    Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.1,
                                g: 0.2,
                                b: 0.3,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment{
                    view: &self.depth_texture.view,
                    depth_ops: Some(wgpu::Operations{
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            render_pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.draw_mesh_instanced(&self.obj_model.meshes[0],&self.obj_model.materials[0], 0..self.instances.len() as u32, &self.camera_bind_group)
        }

        self.queue.submit(Some(encoder.finish()));
        output.present();
        Ok(())
    }
}

impl ApplicationHandler for App<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attributes = Window::default_attributes()
            .with_title("wgpu winit 0.30")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        if self.window.is_none() {
            let window = Arc::new(
                event_loop
                    .create_window(window_attributes)
                    .expect("failed to get window attributes"),
            );
            self.window = Some(window.clone());
            let rt = Runtime::new().expect("Failed to get runtime");
            let state = GameState::new(window);
            let state = rt.block_on(state);
            self.state = Some(state);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if id != self.window.as_ref().unwrap().id() {
            return;
        }
        if !self
            .state
            .as_mut()
            .expect("failed to get input")
            .input(&event)
        {
            match event {
                WindowEvent::CloseRequested => {
                    event_loop.exit();
                }
                WindowEvent::Resized(physical_size) => {
                    self.state.as_mut().unwrap().resize(physical_size);
                }
                WindowEvent::RedrawRequested => {
                    self.state.as_mut().unwrap().update();
                    match self.state.as_mut().unwrap().render() {
                        Ok(_) => {
                            self.state.as_mut().unwrap().update();
                        }
                        Err(wgpu::SurfaceError::Lost) => {
                            let size = self.state.as_mut().unwrap().size;
                            self.state.as_mut().unwrap().resize(size);
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                        Err(e) => eprintln!("{:?}", e),
                    }
                    self.window
                        .as_mut()
                        .expect("failed to get window")
                        .request_redraw();
                }
                _ => (),
            }
        }
    }
}
impl InstanceRaw {
    fn vertex_buffer_layout() -> wgpu::VertexBufferLayout<'static> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<InstanceRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 8]>() as wgpu::BufferAddress,
                    shader_location: 7,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 12]>() as wgpu::BufferAddress,
                    shader_location: 8,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}


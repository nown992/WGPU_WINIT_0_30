use std::io::{BufReader, Cursor};
use wgpu::util::DeviceExt;
use crate::{model, texture};

pub async fn load_string(file_name: &str) -> anyhow::Result<String>{
    let path = std::path::Path::new(env!("OUT_DIR"))
        .join("res")
        .join(file_name);
    let txt = std::fs::read_to_string(path)?;
    Ok(txt)
}

pub async fn load_binary(file_name: &str) -> anyhow::Result<Vec<u8>>{
    let path = std::path::Path::new(env!("OUT_DIR"))
        .join("res")
        .join(file_name);
    let data = std::fs::read(path)?;
    Ok(data)
}

pub async fn load_texture(
    file_name: &str,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> anyhow::Result<texture::Texture> {
    let data = load_binary(file_name).await?;
    texture::Texture::from_bytes(device, queue, &data, file_name)
}

pub async fn load_model(
    file_name:&str,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
) -> anyhow::Result<model::Model>{
    // generate file path as a string
    let obj_text = load_string(file_name).await?;
    // wraps the memory in a cursor
    let obj_cursor = Cursor::new(obj_text);
    // loads the cursor into a buffer to decrease reads for performance
    let mut obj_reader = BufReader::new(obj_cursor);

    let (models, obj_materials) = tobj::load_obj_buf_async(&mut obj_reader, &tobj::LoadOptions{
        triangulate: true,
        single_index: true,
        ..Default::default()
    },
    //material loader portion of the function
    |p| async move {
            //file path as string
            let mat_text = load_string(&p).await.unwrap();
            //load materal from BufReader from file path generated above
            tobj::load_mtl_buf(&mut BufReader::new(Cursor::new(mat_text)))
        },
    )
    .await?;

    let mut materials = Vec::new();
    for material in obj_materials? {
        //get diffuse texture name from material iter and load appropriate texture
        let diffuse_texture = load_texture(&material.diffuse_texture, device, queue).await?;
        //chuck it into a bind group 
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor{
            layout,
            label: None,
            entries: &[
                wgpu::BindGroupEntry{
                    binding:0,
                    resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1, 
                    resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                },
            ],
        });
        //return the materials struct
        materials.push(model::Material{
            name: material.name,
            diffuse_texture,
            bind_group,
        })
    }
//get our meshes of 
    let meshes = models.into_iter()
        .map(|model| {
            //positions are a flattened vec in tobj. len/3 to get number of xyz vertices 
            let vertices = (0..model.mesh.positions.len()/3)
                .map(|vertex|{
                    //positions is a flat array so iterate over it to get [x,y,z], if statement
                    //will define normal as centre coords if not defined. 
                    if model.mesh.normals.is_empty(){
                    model::ModelVertex{
                        position: [
                            model.mesh.positions[vertex*3],
                            model.mesh.positions[vertex*3+1],
                            model.mesh.positions[vertex*3+2],
                        ],
                        tex_coords:[model.mesh.texcoords[vertex*2], 1.0 - model.mesh.texcoords[vertex*2+1]],
                        normal:[0.0,0.0,0.0],
                    }
                } else{
                    model::ModelVertex{
                        position: [
                            model.mesh.positions[vertex*3],
                            model.mesh.positions[vertex*3+1],
                            model.mesh.positions[vertex*3+2],
                        ],
                        tex_coords: [model.mesh.texcoords[vertex*2], 1.0 - model.mesh.texcoords[vertex*2+1]],
                        normal: [
                            model.mesh.normals[vertex*3],
                            model.mesh.normals[vertex*3+1],
                            model.mesh.normals[vertex*3+2],
                        ],
                    }
                    }
                })
            .collect::<Vec<_>>();
// chuck the vertices vec into a vertex buffer.
            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor{
                label: Some(&format!("{:#?} Vertex Buffer", file_name)),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
// index buffers from the mesh indices. 
            let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor{
                label: Some(&format!("{:#?} Index Buffer", file_name)),
                contents:bytemuck::cast_slice(&model.mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
// return the mesh struct into a vec
            model::Mesh{
                name: file_name.to_string(),
                vertex_buffer,
                index_buffer,
                num_elements: model.mesh.indices.len() as u32,
                material: model.mesh.material_id.unwrap_or(0),
            }
        })
    .collect::<Vec<_>>();
//return the Ok result from trying to load the model 
    Ok(model::Model{meshes, materials})
}


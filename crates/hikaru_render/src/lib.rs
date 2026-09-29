// crates/hikaru_render/src/lib.rs

//! Base de render GPU para Hikaru.
//!
//! Este crate existe para agregar wgpu al workspace de forma aislada, sin
//! tocar la superficie de `hikaru_gui` ni el stack de GPUI Kit.
//!
//! # Por qué un crate propio
//!
//! GPUI Kit ya depende de wgpu por dentro: `gpui-kit` -> `gpui-pre-*` ->
//! `gpui-pre-wgpu` -> `wgpu 29.0.4`. Eso significa que wgpu **ya se está
//! compilando** en el binario de Hikaru. Al declarar acá la misma versión
//! exacta, cargo unifica las dos dependencias y hay una sola copia de la
//! biblioteca en el grafo de compilación: no se duplica el tiempo de build ni
//! el peso del binario.
//!
//! Si en algún momento la versión declarada acá se separa de la que usa
//! `gpui-pre-wgpu`, cargo pasa a compilar dos copias de wgpu. Los tipos dejan
//! de ser intercambiables entre crates y aparecen errores confusos del tipo
//! `expected wgpu::Device, found wgpu::Device`. El `wgpu` del
//! `Cargo.toml` raíz es la fuente de verdad; hay que revisarlo si se actualiza
//! GPUI Kit.
//!
//! # Estado actual
//!
//! - [`context`]: ciclo de vida de la GPU, formatos de render y readback.
//! - [`quad`]: quads 2D texturizados, para los sprite sheets de los controles.
//! - [`mesh`]: pipeline 3D inicial, para la malla de la Wavetable.
//! - [`camera`]: matemática de cámara (órbita, perspectiva, view-projection).
//! - [`knob`]: knob 3D, un disco con relieve que gira con el valor.
//!
//! Todos los renderers son **offscreen**: dibujan a texturas en memoria, sin
//! ventana asociada. Para llevar el resultado a pantalla, [`context`] ofrece
//! [`GpuContext::read_color_rgba8`], que devuelve los píxeles del target como
//! RGBA8 y es lo que usa `hikaru_gui` para componer el render 3D dentro del
//! layout 2D de GPUI Kit.

pub mod camera;
pub mod context;
pub mod knob;
pub mod mesh;
pub mod quad;

pub use camera::{Camera, Mat4};
pub use knob::{
    upload_knob, KnobMesh, KnobMeshParams, KnobRenderer, KnobUniforms, KNOB_RESOLUTION,
};
pub use context::{GpuContext, ReadbackError, RenderTarget};
pub use mesh::{GpuMesh, MeshRenderer, MeshUniforms, WavetableMesh, WavetableMeshParams};
pub use quad::{
    QuadError, QuadInstance, QuadRenderer, SpriteLayout, SpriteSheet, SpriteSheetResources,
    UvRect,
};

/// Reexport de wgpu para que los crates consumidores no tengan que declararlo
/// por separado ni arriesgarse a terminar con otra versión.
pub use wgpu;

/// Versión de wgpu con la que se compiló este crate.
///
/// Es el valor que hay que comparar contra el del `Cargo.toml` raíz si alguna
/// vez se actualiza GPUI Kit: si dejan de coincidir, cargo compila dos copias
/// de wgpu y los tipos dejan de ser intercambiables entre crates.
pub const WGPU_VERSION: &str = "29.0.4";


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
//! - [`context`]: ciclo de vida de la GPU y formatos de render.
//! - [`quad`]: quads 2D texturizados, para los sprite sheets de los controles.
//! - [`mesh`]: pipeline 3D inicial, para la malla de la Wavetable.
//!
//! Todos los renderers son **offscreen**: dibujan a texturas en memoria, sin
//! ventana asociada. La presentación en pantalla queda para una etapa posterior,
//! cuando se defina cómo se integra `hikaru_render` con la superficie que ya
//! maneja GPUI Kit.

pub mod context;
pub mod mesh;
pub mod quad;

pub use context::{GpuContext, RenderTarget};
pub use mesh::{GpuMesh, MeshRenderer, MeshUniforms, WavetableMesh};
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


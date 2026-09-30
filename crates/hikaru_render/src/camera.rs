// crates/hikaru_render/src/camera.rs

//! Matemática de cámara para el pipeline 3D.
//!
//! # Por qué vive acá y no en el shader
//!
//! La matriz `view_proj` de [`crate::mesh::MeshUniforms`] se arma en CPU y se
//! sube en cada draw. Hacerlo del lado del renderer significa que:
//!
//! - el WGSL queda mínimo (una sola multiply por vértice),
//! - y la cámara se puede testear sin GPU, que es la única forma de verificar
//!   que una perspectiva no termina con la malla detrás del ojo o aplastada.
//!
//! # Convenciones
//!
//! Todo es **right-handed, mirando por -Z**, que es lo que espera WebGPU: el
//! near plane cae en `z = 0` y el far plane en `z = 1` del NDC, no en `[-1, 1]`
//! como en OpenGL. Las matrices se guardan en **columna mayor** (`m[columna]`
//! con 4 filas adentro), que es el layout que consume `mat4x4<f32>` en WGSL
//! sin transponer nada.

/// Matriz 4x4 en columna mayor: `m[columna][fila]`.
pub type Mat4 = [[f32; 4]; 4];

/// Matriz identidad.
pub fn identity() -> Mat4 {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// Producto de matrices `a * b`, en el mismo orden que se aplica a un vector.
///
/// Con columna mayor, `(a * b)[c][r] = sum_k a[k][r] * b[c][k]`. El orden
/// importa y es la fuente de error clásica: `projection * view` y
/// `view * projection` dan matrices distintas, y sólo una deja la malla dentro
/// del frustum.
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0f32; 4]; 4];
    for (column, out_column) in out.iter_mut().enumerate() {
        for (row, value) in out_column.iter_mut().enumerate() {
            let mut sum = 0.0;
            for k in 0..4 {
                sum += a[k][row] * b[column][k];
            }
            *value = sum;
        }
    }
    out
}

/// Aplica una matriz a un punto (con `w` implícito en 1).
///
/// Devuelve además el `w` resultante: es lo que permite el *clipping* trivial
/// al dividir por `w` para llegar a NDC.
pub fn transform_point(m: &Mat4, point: [f32; 3]) -> ([f32; 3], f32) {
    let x = m[0][0] * point[0] + m[1][0] * point[1] + m[2][0] * point[2] + m[3][0];
    let y = m[0][1] * point[0] + m[1][1] * point[1] + m[2][1] * point[2] + m[3][1];
    let z = m[0][2] * point[0] + m[1][2] * point[1] + m[2][2] * point[2] + m[3][2];
    let w = m[0][3] * point[0] + m[1][3] * point[1] + m[2][3] * point[2] + m[3][3];
    ([x, y, z], w)
}

/// Matriz de proyección en perspectiva.
///
/// `fov_y` en radianes, `aspect` = ancho / alto. El rango de profundidad es el
/// de WebGPU: `0` en el near plane y `1` en el far plane.
///
/// Un `fov_y <= 0` o un `near <= 0` degenerarían la división; en vez de dejar
/// que la malla desaparezca con NaN silenciosos, se cae a un fov de 45° y un
/// near de 0.1, que es lo único razonable para una vista de viewport.
pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let fov_y = if fov_y.is_finite() && fov_y > f32::EPSILON { fov_y } else { DEFAULT_FOV_Y };
    let aspect = if aspect.is_finite() && aspect > f32::EPSILON { aspect } else { 1.0 };
    let near = if near.is_finite() && near > f32::EPSILON { near } else { 0.1 };
    // Un far igual o menor que el near invierte el rango de profundidad y
    // wgpu descarta los triángulos que quedan con `z` fuera de [0, 1].
    let far = if far.is_finite() && far > near { far } else { near * 1000.0 };

    let focal = 1.0 / (fov_y * 0.5).tan();

    [
        [focal / aspect, 0.0, 0.0, 0.0],
        [0.0, focal, 0.0, 0.0],
        [0.0, 0.0, far / (near - far), -1.0],
        [0.0, 0.0, far * near / (near - far), 0.0],
    ]
}

/// Matriz de vista `look_at` (a diferencia de `look_to`, que es la inversa).
///
/// `up` no necesita estar normalizado ni ser perpendicular a la dirección: el
/// proceso Gram-Schmidt de abajo lo ortonormaliza. `up` degenerado (paralelo a
/// la dirección de vista) se reemplaza por un eje para que la matriz no quede
/// con una base singular.
pub fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> Mat4 {
    let forward = normalize(sub(target, eye));
    let mut side = cross(forward, up);
    if length(side) <= f32::EPSILON {
        // La dirección de vista es paralela a `up`: se elige cualquier eje no
        // colineal para recuperar una base.
        side = cross(forward, [0.0, 0.0, 1.0]);
        if length(side) <= f32::EPSILON {
            side = cross(forward, [1.0, 0.0, 0.0]);
        }
    }
    let side = normalize(side);
    // `up` se re-deriva de la base derecha para que sea exactamente
    // perpendicular: usar el `up` crudo introduce error de escala en la vista.
    let up = cross(side, forward);

    [
        [side[0], up[0], -forward[0], 0.0],
        [side[1], up[1], -forward[1], 0.0],
        [side[2], up[2], -forward[2], 0.0],
        [-dot(side, eye), -dot(up, eye), dot(forward, eye), 1.0],
    ]
}

/// Cámara orbital: la que usa la vista de la Wavetable.
///
/// Los tres parámetros que la UI expone son los de una órbita clásica, y están
/// en las unidades en las que la UI los edita: `yaw` y `pitch` en radianes,
/// `distance` en las mismas unidades locales de la malla (la cinta mide
/// 320 x 200 por defecto, así que la distancia útil es del orden de 300-600).
///
/// Se guarda el `target` y no un `eye` porque un editor necesita poder
/// desplazar el punto de mira sin rehacer la órbita: el `eye` se deriva.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Rotación alrededor del eje Y, en radianes. `0` mira la cinta de frente
    /// (a lo largo de -Z).
    pub yaw: f32,
    /// Rotación alrededor del eje X, en radianes. Positivo levanta la vista.
    pub pitch: f32,
    /// Distancia al `target`. `0` o menor degeneraría la proyección.
    pub distance: f32,
    /// Punto al que mira la cámara.
    pub target: [f32; 3],
    /// Apertura vertical del frustum, en radianes.
    pub fov_y: f32,
    /// Plano near, en unidades locales.
    pub near: f32,
    /// Plano far, en unidades locales.
    pub far: f32,
}

/// FOV vertical por defecto: 45°, un valor intermedio que evita la distorsión
/// de un gran angular sin comprimir tanto la cinta que se vea plana.
pub const DEFAULT_FOV_Y: f32 = std::f32::consts::FRAC_PI_4;

/// Inclinación de la vista del visor 3D: 30° sobre el horizonte.
pub const VIEWER_PITCH: f32 = std::f32::consts::FRAC_PI_6;

/// Fracción de la pantalla que debe ocupar la malla, en NDC.
///
/// 0.8 deja un 10% de aire por lado. Más que 0.85 y la caja empieza a rozar el
/// borde al inclinar la cámara, porque las esquinas de arriba se proyectan más
/// afuera que el centro.
pub const DEFAULT_FILL: f32 = 0.85;

impl Default for Camera {
    /// La cámara del visor: posición fija, de frente e inclinada 30°.
    ///
    /// Es la que usa el editor de Wavetable. La perspectiva es fija a propósito:
    /// los controles de yaw/pitch/reset que había antes no aportan nada a leer
    /// un ciclo y le quitaban espacio a la forma de onda, que es lo único que el
    /// panel existe para mostrar.
    fn default() -> Self {
        Self::wavetable_viewer()
    }
}

impl Camera {
    /// La cámara del visor 3D de la Wavetable: fija, de frente e inclinada 30°.
    ///
    /// # Por qué estos valores
    ///
    /// **Yaw 0°**: la cinta se lee de frente. El volumen lo aporta la
    /// inclinación, no el giro lateral; con yaw la forma de onda queda
    /// escorzada y hay que rotar la cabeza mentalmente para leer un ciclo, que
    /// es justo lo que un selector de wavetable no puede pedir.
    ///
    /// **Pitch 30°** ([`VIEWER_PITCH`]): es el ángulo donde la cinta se lee como
    /// volumen sin que el `thickness` se proyecte de perfil. Por debajo de unos
    /// 20° se ve una línea y se pierde el canto; por encima de 45° la onda se
    /// aplana tanto que la forma deja de leerse como forma de onda.
    ///
    /// **Fov 45°**: con un fov corto la perspectiva se vuelve casi ortográfica y
    /// la cinta pierde la sensación de cinta; con uno largo, las esquinas del
    /// lazo se curvan de más y parece un tubo.
    ///
    /// La distancia es un valor de arranque: [`Camera::fit_to_box`] la recalcula
    /// contra el tamaño real de la malla y del viewport, así que una tabla más
    /// grande se encuadra sola.
    pub fn wavetable_viewer() -> Self {
        Self {
            yaw: 0.0,
            pitch: VIEWER_PITCH,
            distance: 420.0,
            target: [0.0, 0.0, 0.0],
            fov_y: DEFAULT_FOV_Y,
            near: 1.0,
            far: 4000.0,
        }
    }

    /// Cámara de frente, sin inclinación.
    ///
    /// Es la que se usa en los tests y como estado neutro: con `view_proj`
    /// identidad el WGSL seguiría dibujando, así que una configuración sin
    /// cámara no debe parecerse a una cámara rota.
    pub fn front() -> Self {
        Self { yaw: 0.0, pitch: 0.0, ..Self::wavetable_viewer() }
    }

    /// Cámara de un knob: mira el disco de frente, desde arriba y de cerca.
    ///
    /// El knob tiene radio 1 ([`crate::knob::KnobMeshParams`]) y la cámara se
    /// ubica a una distancia que lo deja entrar con margen en el fov: a 2.6
    /// unidades, el disco ocupa algo menos de la mitad del alto del viewport,
    /// que es como se ve un knob en un panel de verdad.
    pub fn knob() -> Self {
        Self {
            yaw: 0.0,
            // Apenas inclinado: si el disco se ve perfectamente recto pierde
            // el canto y deja de leerse como un volumen.
            pitch: 0.22,
            distance: 2.6,
            target: [0.0, 0.0, 0.0],
            fov_y: DEFAULT_FOV_Y,
            near: 0.1,
            far: 40.0,
        }
    }

    /// Reencuadra la cámara para que una caja de `half_extents` entre completa
    /// en el viewport, ocupando [`DEFAULT_FILL`] de la pantalla.
    ///
    /// Ver [`Camera::fit_to_box_with_fill`] para el algoritmo.
    pub fn fit_to_box(&mut self, half_extents: [f32; 3], aspect: f32) {
        self.fit_to_box_with_fill(half_extents, aspect, DEFAULT_FILL);
    }

    /// Reencuadra la cámara para que una caja de `half_extents` ocupe `fill` de
    /// la pantalla, en fracción del semiancho o del semialto del viewport.
    ///
    /// `half_extents` son los semiejes de la caja: `[x, y, z]` en unidades
    /// locales. El de Z importa desde que la vista apila los ciclos de la
    /// wavetable: sin él, una pila de 24 ciclos se sale por abajo del encuadre
    /// aunque el ancho y el alto entran de sobra.
    ///
    /// # Por qué proyectar las esquinas y no usar una esfera
    ///
    /// La primera versión encuadraba con la **esfera envolvente** de la caja:
    /// `distance = radio / sin(fov/2) * margen`. Es simple y nunca recorta, pero
    /// es muy holgada: el radio de la esfera es la diagonal de la caja, y el
    /// ancho y el alto reales son bastante menores. Con la caja de la vista
    /// (320 x 120 x 110) el radio da 180 y la distancia qued�� en 460, con la
    /// malla ocupando poco más de la mitad del ancho del visor. Se veía el
    /// objeto diminuto y centrado, que es justo lo que había que corregir.
    ///
    /// Acá se proyectan las ocho esquinas con la matriz real y se mide cuánto
    /// del viewport ocupa la peor de ellas. Como el tamaño proyectado es
    /// inversamente proporcional a la distancia para un objeto lejano, cada
    /// vuelta corrige `distance *= occupy / fill`. Con la cámara inclinada las
    /// esquinas de arriba están más cerca que las de abajo, y por eso hace falta
    /// iterar en vez de resolverlo con una fórmula cerrada.
    ///
    /// Tres vueltas alcanzan: la relación es una hipérbola suave y a la tercera
    /// pasada el error es de milésimas.
    pub fn fit_to_box_with_fill(&mut self, half_extents: [f32; 3], aspect: f32, fill: f32) {
        let aspect = if aspect.is_finite() && aspect > f32::EPSILON { aspect } else { 1.0 };
        let fill = if fill.is_finite() && (0.1..=1.0).contains(&fill) { fill } else { DEFAULT_FILL };

        let (hx, hy, hz) = (half_extents[0].max(0.0), half_extents[1].max(0.0), half_extents[2].max(0.0));
        if hx <= f32::EPSILON && hy <= f32::EPSILON && hz <= f32::EPSILON {
            return;
        }

        for _ in 0..3 {
            // El near y el far se mueven con la cámara: si la distancia baja lo
            // suficiente para que la caja pase del near plane, se la ve cortada
            // por el plano, y un near fijo deja de ser válido al reencuadrar.
            self.near = (self.distance * 0.01).max(f32::EPSILON);
            self.far = (self.distance * 4.0).max(self.near * 1000.0);

            let view_proj = self.view_proj(aspect);

            // La peor esquina: la más cercana al borde del viewport. Se mide en
            // NDC, donde 1.0 es el borde, así que el valor es directamente la
            // fracción de pantalla que ocupa la caja.
            let mut occupy = 0.0f32;
            for x in [-hx, hx] {
                for y in [-hy, hy] {
                    for z in [-hz, hz] {
                        let (clip, w) = transform_point(&view_proj, [x, y, z]);
                        // `w <= 0` es un punto detrás del ojo o en el plano cercano:
                        // se ignora porque proyectarlo no dice nada del tamaño.
                        if !w.is_finite() || w <= f32::EPSILON {
                            continue;
                        }
                        let ndc_x = (clip[0] / w).abs();
                        let ndc_y = (clip[1] / w).abs();
                        if !ndc_x.is_finite() || !ndc_y.is_finite() {
                            continue;
                        }
                        occupy = occupy.max(ndc_x).max(ndc_y);
                    }
                }
            }

            if !occupy.is_finite() || occupy <= f32::EPSILON {
                // La caja entera quedó detrás del ojo: no hay distancia que la
                // arregle, así que se vuelve a una que sí la contiene.
                self.distance = (hx.hypot(hy).max(hy).max(hz) * 4.0).max(1.0);
                return;
            }

            self.distance = (self.distance * occupy / fill).clamp(1.0, 100_000.0);
        }
    }

    /// Posición de la cámara derivada de la órbita.
    pub fn eye(&self) -> [f32; 3] {
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();

        [
            self.target[0] + self.distance * cos_pitch * sin_yaw,
            self.target[1] + self.distance * sin_pitch,
            self.target[2] + self.distance * cos_pitch * cos_yaw,
        ]
    }

    /// Matriz de vista de esta cámara.
    pub fn view(&self) -> Mat4 {
        look_at(self.eye(), self.target, [0.0, 1.0, 0.0])
    }

    /// Matriz de proyección para un viewport de `aspect` (ancho / alto).
    pub fn projection(&self, aspect: f32) -> Mat4 {
        perspective(self.fov_y, aspect, self.near, self.far)
    }

    /// `projection * view`: lo que se sube a los uniforms.
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        mul(&self.projection(aspect), &self.view())
    }

    /// Dirección de la luz en espacio local, a partir de una luz fija en
    /// espacio de vista.
    ///
    /// El shader interpola la normal tal cual (no la pasa por la view matrix) y
    /// la compara contra este vector. Por eso la luz hay que *des*-rotar con la
    /// cámara: si se mandara el vector de vista sin transformar, al girar el
    /// `yaw` la cara iluminada se quedaría pegada a la cámara y la cinta
    /// parecería plana.
    pub fn local_light_dir(&self, view_space_light: [f32; 3]) -> [f32; 3] {
        // La rotación inversa de una órbita yaw (Y) + pitch (X) es pitch (-X)
        // seguido de yaw (-Y).
        let (sin_pitch, cos_pitch) = (-self.pitch).sin_cos();
        let (sin_yaw, cos_yaw) = (-self.yaw).sin_cos();

        let [x, y, z] = view_space_light;

        // Rotación en X.
        let [x, y1, z1] = [x, y * cos_pitch - z * sin_pitch, y * sin_pitch + z * cos_pitch];
        // Rotación en Y.
        let rotated = [x * cos_yaw + z1 * sin_yaw, y1, -x * sin_yaw + z1 * cos_yaw];

        // Las dos rotaciones preservan la longitud, pero el vector de entrada
        // puede no venir normalizado (la UI usa un color de luz arbitrario) y
        // el shader multiplica por `light_intensity` *después* de normalizar.
        // Devolverlo normalizado mantiene el brillo constante al girar.
        normalize(rotated)
    }

    /// Luces que produce la cámara, listas para [`crate::mesh::MeshUniforms`].
    pub fn uniforms(
        &self,
        aspect: f32,
        tint: [f32; 4],
    ) -> crate::mesh::MeshUniforms {
        crate::mesh::MeshUniforms {
            view_proj: self.view_proj(aspect),
            light_dir: self.local_light_dir([0.4, 0.6, 1.0]),
            light_intensity: 1.0,
            tint,
        }
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length(v: [f32; 3]) -> f32 {
    dot(v, v).sqrt()
}

/// Normaliza un vector 3D, con un eje de reserva para el vector nulo.
pub fn normalize(v: [f32; 3]) -> [f32; 3] {
    let length = length(v);
    if !length.is_finite() || length <= f32::EPSILON {
        return [0.0, 0.0, 1.0];
    }
    [v[0] / length, v[1] / length, v[2] / length]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un punto delante de la cámara, a `depth` unidades sobre -Z local.
    fn in_front(depth: f32) -> [f32; 3] {
        [0.0, 0.0, -depth]
    }

    fn ndc(point: [f32; 3], view_proj: &Mat4) -> [f32; 3] {
        let (clip, w) = transform_point(view_proj, point);
        [clip[0] / w, clip[1] / w, clip[2] / w]
    }

    #[test]
    fn identity_is_a_no_op() {
        let (out, w) = transform_point(&identity(), [1.0, 2.0, 3.0]);
        assert_eq!(out, [1.0, 2.0, 3.0]);
        assert_eq!(w, 1.0);
    }

    #[test]
    fn multiplication_is_not_commutative() {
        // projection * view y view * projection dan matrices distintas: por eso
        // el orden está fijado en `view_proj` y no se deja a criterio de quien
        // llama.
        let camera = Camera::default();
        let forward = mul(&camera.projection(1.6), &camera.view());
        let backward = mul(&camera.view(), &camera.projection(1.6));
        assert_ne!(forward, backward);
    }

    #[test]
    fn a_point_in_front_lands_inside_the_frustum() {
        // Éste es el test que importa: con la matriz mal compuesta o con el
        // signo de `w` al revés, la malla se dibuja pero fuera de pantalla y
        // no hay ningún error de wgpu que lo delate.
        let camera = Camera::default();
        let view_proj = camera.view_proj(16.0 / 9.0);
        let center = ndc([0.0, 0.0, 0.0], &view_proj);

        assert!(center[0].abs() < 1.0, "el origen se fue de pantalla en x: {center:?}");
        assert!(center[1].abs() < 1.0, "el origen se fue de pantalla en y: {center:?}");
        // z = 0 en el near plane, 1 en el far: el near de la cámara está
        // delante del origen, así que el origen cae dentro del rango.
        assert!((0.0..=1.0).contains(&center[2]), "z fuera de [0, 1]: {center:?}");
    }

    #[test]
    fn the_default_camera_frames_the_whole_wavetable_box() {
        // La cinta se construye en 260x100 (ver `WavetableMeshParams`), así que
        // ninguna esquina de esa caja puede caer fuera del viewport con la
        // cámara por defecto.
        let camera = Camera::default();
        let aspect = 1.6;
        let view_proj = camera.view_proj(aspect);

        for x in [-130.0f32, 130.0] {
            for y in [-50.0f32, 50.0] {
                for z in [-40.0f32, 40.0] {
                    let point = ndc([x, y, z], &view_proj);
                    assert!(
                        point[0].abs() <= 1.0 && point[1].abs() <= 1.0,
                        "la esquina ({x}, {y}, {z}) quedó fuera de pantalla: {point:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn fit_to_box_grows_the_distance_for_bigger_meshes() {
        let mut camera = Camera::default();
        camera.fit_to_box([160.0, 100.0, 12.0], 1.6);
        let close = camera.distance;
        camera.fit_to_box([1600.0, 1000.0, 12.0], 1.6);
        assert!(camera.distance > close * 5.0, "el encuadre no creció con la caja");

        // Después del ajuste, la caja entra.
        let view_proj = camera.view_proj(1.6);
        let corner = ndc([1600.0, 1000.0, 0.0], &view_proj);
        assert!(corner[0].abs() <= 1.0 && corner[1].abs() <= 1.0, "{corner:?}");
    }

    /// Fracción de pantalla que ocupa la caja tras el encuadre.
    fn occupancy(camera: &Camera, half: [f32; 3], aspect: f32) -> f32 {
        let view_proj = camera.view_proj(aspect);
        let mut worst = 0.0f32;
        for x in [-half[0], half[0]] {
            for y in [-half[1], half[1]] {
                for z in [-half[2], half[2]] {
                    let point = ndc([x, y, z], &view_proj);
                    worst = worst.max(point[0].abs()).max(point[1].abs());
                }
            }
        }
        worst
    }

    #[test]
    fn the_wavetable_box_fills_the_viewport() {
        // El requisito aesthetic del visor: la pila de ciclos tiene que ocupar
        // la mayor parte del panel, no flotar chica en el centro. Antes el
        // encuadre por esfera envolvente la dejaba en menos de la mitad.
        let mut camera = Camera::wavetable_viewer();
        let half = [130.0, 50.0, 40.0];
        camera.fit_to_box(half, 320.0 / 200.0);

        let occupy = occupancy(&camera, half, 320.0 / 200.0);
        assert!(
            (occupy - DEFAULT_FILL).abs() < 0.03,
            "la malla ocupa {occupy:.2} del viewport y se pidió {DEFAULT_FILL}"
        );
    }

    #[test]
    fn fit_to_box_with_fill_honors_a_smaller_fill() {
        // Un fill chico tiene que alejar la cámara, no cambiar el fov: es el
        // encuadre, no la perspectiva, lo que se está ajustando.
        let half = [130.0, 50.0, 40.0];
        let mut tight = Camera::wavetable_viewer();
        let mut loose = Camera::wavetable_viewer();
        tight.fit_to_box_with_fill(half, 1.6, 0.9);
        loose.fit_to_box_with_fill(half, 1.6, 0.45);

        assert!(loose.distance > tight.distance, "el fill chico no aleja la cámara");
        assert!((occupancy(&loose, half, 1.6) - 0.45).abs() < 0.03);
        assert!((occupancy(&tight, half, 1.6) - 0.9).abs() < 0.03);
    }

    #[test]
    fn fit_to_box_keeps_the_mesh_in_front_of_the_near_plane() {
        // El near se recalcula junto con la distancia. Si quedara fijo, un
        // encuadre muy cerrado pondría la caja contra el near plane y se vería
        // cortada por el plano, que es un recorte sin ningún error de wgpu.
        let mut camera = Camera::wavetable_viewer();
        camera.fit_to_box_with_fill([130.0, 50.0, 40.0], 1.6, 0.78);

        let view_proj = camera.view_proj(1.6);
        for x in [-130.0, 130.0] {
            for y in [-50.0, 50.0] {
                for z in [-40.0, 40.0] {
                    let (_, w) = transform_point(&view_proj, [x, y, z]);
                    assert!(w > 0.0, "la esquina ({x}, {y}, {z}) quedó en w <= 0");
                    let point = ndc([x, y, z], &view_proj);
                    assert!((0.0..=1.0).contains(&point[2]), "z fuera de [0, 1]: {point:?}");
                }
            }
        }
    }

    #[test]
    fn fit_to_box_survives_a_degenerate_box() {
        // Una caja plana o de tamaño cero no puede producir distancia infinita
        // ni NaN: el encoder manda un waveform de un sample y la malla sale de
        // dimensión cero.
        for half in [[0.0, 0.0, 0.0], [160.0, 0.0, 0.0], [0.0, 0.0, 55.0]] {
            let mut camera = Camera::wavetable_viewer();
            camera.fit_to_box(half, 1.6);
            assert!(
                camera.distance.is_finite() && camera.distance > 0.0,
                "caja {half:?} dejó la distancia en {}",
                camera.distance
            );
        }
    }

    #[test]
    fn fit_to_box_backs_off_for_a_deep_stack_of_cycles() {
        // La pila de ciclos ocupa Z. Encuadrarla con la profundidad de una sola
        // cinta la deja salir por abajo: el ajuste tiene que crecer con Z.
        let mut flat = Camera::default();
        flat.fit_to_box([130.0, 50.0, 12.0], 1.6);

        let mut deep = Camera::default();
        deep.fit_to_box([130.0, 50.0, 40.0], 1.6);

        assert!(deep.distance > flat.distance, "una pila mas profunda no se aleja menos");
    }

    #[test]
    fn fit_to_box_survives_a_degenerate_aspect() {
        // Un viewport de ancho cero aparece cuando el panel todavía no tiene
        // layout. No puede producir NaN en la matriz.
        let mut camera = Camera::default();
        camera.fit_to_box([160.0, 100.0, 12.0], 0.0);
        assert!(camera.distance.is_finite() && camera.distance > 0.0);
    }

    #[test]
    fn near_plane_maps_to_zero_and_far_to_one() {
        // La convención de profundidad de WebGPU. Si esto se rompe, la malla
        // desaparece en release (el clipping la descarta) pero se ve bien en
        // debug, que es el peor síntoma posible.
        let projection = perspective(DEFAULT_FOV_Y, 1.0, 10.0, 1000.0);
        let near = ndc(in_front(10.0), &projection);
        let far = ndc(in_front(1000.0), &projection);
        assert!((near[2] - 0.0).abs() < 1e-5, "near no quedó en z = 0: {near:?}");
        assert!((far[2] - 1.0).abs() < 1e-5, "far no quedó en z = 1: {far:?}");
    }

    #[test]
    fn everything_behind_the_camera_is_discarded() {
        // `w` negativo es lo que activa el clipping del vértice. Si la
        // proyección perdiera el signo de `w`, la cinta se vería desde atrás
        // del ojo, reflejada.
        let projection = perspective(DEFAULT_FOV_Y, 1.0, 1.0, 100.0);
        let (_, w) = transform_point(&projection, [0.0, 0.0, 10.0]);
        assert!(w < 0.0, "un punto detrás de la cámara no debería tener w > 0");
    }

    #[test]
    fn aspect_ratio_stretches_only_the_horizontal_axis() {
        // Con un viewport el doble de ancho, un punto a la misma distancia
        // queda la mitad de lejos del borde: es lo que hace que una cinta se
        // vea con la misma altura y más aire a los lados.
        let projection = perspective(DEFAULT_FOV_Y, 2.0, 1.0, 100.0);
        let (clip, w) = transform_point(&projection, [0.5, 0.0, -10.0]);
        assert!((clip[0] / w).abs() < 0.5);
    }

    #[test]
    fn look_at_keeps_the_eye_fixed() {
        // Invariante de una matriz de vista: el ojo mapea al origen.
        let eye = [10.0, 20.0, 30.0];
        let view = look_at(eye, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let (out, w) = transform_point(&view, eye);
        assert!((out[0]).abs() < 1e-4 && (out[1]).abs() < 1e-4 && (out[2]).abs() < 1e-4);
        assert!((w - 1.0).abs() < 1e-4);
    }

    #[test]
    fn look_at_survives_an_up_parallel_to_the_view_direction() {
        // Mirar straight down con `up` vertical: la base derecha se anula y la
        // matriz queda singular. El fallback por eje tiene que entrar.
        let view = look_at([0.0, 10.0, 0.0], [0.0; 3], [0.0, 1.0, 0.0]);
        let (out, _) = transform_point(&view, [0.0, 10.0, 0.0]);
        assert!(out.iter().all(|value| value.is_finite()), "{out:?}");
    }

    #[test]
    fn a_degenerate_projection_falls_back_instead_of_returning_nan() {
        // fov 0, near 0 o far < near: la división explotaría. Tiene que salir
        // una matriz usable.
        for (fov, near, far) in [(0.0f32, 1.0f32, 100.0f32), (1.0, 0.0, 100.0), (1.0, 10.0, 5.0)] {
            let projection = perspective(fov, 1.0, near, far);
            assert!(projection.iter().all(|column| column.iter().all(|v| v.is_finite())));
        }
    }

    #[test]
    fn rotating_the_camera_actually_moves_the_eye() {
        // Si `eye` no dependiera de yaw/pitch, la vista parecería fija y el
        // editor no serviría para Inclinar la superficie.
        let mut camera = Camera::default();
        let before = camera.eye();
        camera.yaw += 0.8;
        let after = camera.eye();
        assert!((before[0] - after[0]).abs() > 1.0, "el yaw no movió la cámara");

        camera.yaw = 0.0;
        camera.pitch = 0.0;
        // Con yaw y pitch en 0, la cámara queda en +Z sobre el target.
        let axis = camera.eye();
        assert!((axis[0]).abs() < 1e-4 && (axis[2] - camera.distance).abs() < 1e-3);
    }

    #[test]
    fn the_light_follows_the_camera_in_local_space() {
        // Al girar la cámara, la luz en espacio local tiene que cambiar: si no
        // cambia, el sombreado queda clavado a la vista y la cinta se ve plana
        // por más que se incline.
        let mut camera = Camera::default();
        let before = camera.local_light_dir([0.4, 0.6, 1.0]);
        camera.yaw += 1.2;
        let after = camera.local_light_dir([0.4, 0.6, 1.0]);
        assert!((before[0] - after[0]).abs() > 1e-3, "la luz no se des-rotó");

        // Y siempre tiene que seguir siendo un vector unitario: el shader lo
        // normaliza igual, pero un `light_intensity` con escala implícita
        // cambiaría el brillo al girar la cámara.
        let length = dot(after, after).sqrt();
        assert!((length - 1.0).abs() < 1e-4, "la luz dejó de ser unitaria: {length}");
    }

    #[test]
    fn uniforms_carry_the_camera_matrix() {
        let camera = Camera::default();
        let uniforms = camera.uniforms(1.5, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(uniforms.view_proj, camera.view_proj(1.5));
        assert_eq!(uniforms.tint, [1.0, 0.0, 0.0, 1.0]);
    }
}

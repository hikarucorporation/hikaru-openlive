// crates/hikaru_gui/src/views/wavetable_io.rs

//! Lectura de wavetables desde archivos `.wav` de disco.
//!
//! # Qué es una wavetable acá
//!
//! Una tabla de un solo ciclo: los samples tal cual, en orden, con el último
//! pegado al primero. Es el formato que usan casi todos los `.wav` de
//! wavetable que se descargan, y también el que se puede sintetizar al vuelo.
//! Lo que **no** se desarma acá es el formato de tabla multip Frame de Serum o
//! de los packs profesionales: para esos hay que leer el header del plugin, y
//! eso es otro trabajo.
//!
//! Un archivo más largo que un ciclo se trata como varios ciclos encadenados
//! ([`Wavetable::frames`]), que es exactamente el layout de Serum. Así el knob
//! de índice tiene algo que recorrer en cuanto el usuario cargue un archivo en
//! vez de quedar clavado en 1/1.
//!
//! # Lo que NO hace
//!
//! No manda la tabla al motor de audio. El motor recibe su wavetable por
//! referencia al construirse (`AudioEngine::new` toma `&'a [f32]`) y hoy la que
//! tiene la app es un array de ceros, así que lo que se ve en el visor 3D es
//! la wavetable real del usuario pero lo que suena no: eso requiere cambiar la
//! propiedad del oscilador de `hikaru_dsp` y agregar un comando al motor. Está
//! anotado acá para que nadie piense que editar la tabla ya afecta al audio.

use std::path::{Path, PathBuf};

/// Tamaño al que se re-muestrea cualquier tabla.
///
/// 2048 es la resolución de trabajo: por encima de 1024 no se ve diferencia en
/// la cinta 3D (que tiene 256 columnas) y por debajo de 512 los picos se
/// pierden.
pub const TABLE_RESOLUTION: usize = 2048;

/// Samples por ciclo de la tabla, cuando el archivo trae varios ciclos.
///
/// Es [`TABLE_RESOLUTION`] a propósito: la tabla se re-muestrea a esa
/// resolución al cargarla, así que la cinta de un frame y la de otro miden
/// exactamente lo mismo y se pueden apilar en Z sincosturas.
///
/// Es `pub` porque la vista 3D corta la tabla en ciclos con este mismo número:
/// si la malla usara un stride distinto al del parser, mostraría mitades de
/// ciclo y el realce del knob caería entre dos formas.
pub const FRAME_SAMPLES: usize = TABLE_RESOLUTION;

/// Ciclos de la tabla que trae el sintetizador de fábrica.
///
/// 256 es el tamaño de la matriz de las wavetables profesionales, y además el
/// mínimo para que el knob se sienta continuo: por debajo de unos 60, el
/// barrido del `WT POS` se ve como una tanda de saltos entre ciclos.
///
/// 256 ciclos a 2048 muestras son 2 MB de `f32`. Es lo que ocupa la tabla del
/// editor, y no la del archivo: la del editor se regenera al arrancar.
pub const DEFAULT_CYCLES: usize = 256;

/// Armónicos del último ciclo.
///
/// Es un tope duro por dos razones. Una, la tasa: 256 ciclos x 2048 muestras x
/// armónicos es la cuenta de la síntesis aditiva (ver
/// [`Wavetable::default_table`]). Dos, y la principal: por encima de la mitad de
/// la tasa de Nyquist de los 2048 samples el armónico se envuelve y aparece
/// como alias de una frecuencia que no está. 32 queda muy por debajo del límite y
/// ya da un diente de sierra denso, que es de donde saca el brillo la tabla.
const MAX_HARMONICS: usize = 32;

/// Caída del espectro del primer ciclo: la amplitud del armónico `k` es
/// `1 / k^2.4`.
///
/// Poco más de 2 es casi un seno: armónicos altos presentes pero sordos. Con
/// 2.0 el segundo armónico ya queda en la mitad y el timbre ya tiene color.
const DARK_ROLL_OFF: f32 = 2.4;

/// Caída del espectro del último ciclo: `1 / k^1.0`.
///
/// 1.0 es la serie armónica pura, o sea un diente de sierra idealizado. Es el
/// otro extremo del recorrido: del casi-seno al brillo completo.
const BRIGHT_ROLL_OFF: f32 = 1.0;

/// Una wavetable cargada.
///
/// El buffer es la tabla **entera**: `frames` bloques consecutivos de
/// `TABLE_RESOLUTION` samples. Con un solo frame (el caso más común) es
/// indistinguible de un bloque, y con varios es lo que permite que el knob de
/// índice recorra la tabla.
#[derive(Debug, Clone, PartialEq)]
pub struct Wavetable {
    /// Nombre para mostrar. Viene del nombre del archivo.
    pub name: String,
    /// De dónde salió. `None` para la tabla por defecto del sintetizador.
    pub path: Option<PathBuf>,
    /// La tabla entera: `frames * TABLE_RESOLUTION` samples.
    pub samples: Vec<f32>,
    /// Cuántos frames trae la tabla.
    ///
    /// Con un archivo de un solo ciclo es 1, y el knob de índice queda clavado:
    /// es el caso más común y no hay nada que recorrer.
    pub frames: usize,
}

impl Wavetable {
    /// La tabla por defecto de un solo ciclo: un seno.
    ///
    /// Un seno y no ceros a propósito: con una tabla de ceros la cinta 3D es un
    /// plano y el visor parece roto.
    ///
    /// **Un solo ciclo**: con esta tabla el knob `WT POS` no tiene recorrido y
    /// queda desactivado. Para la tabla con la que arranca el sintetizador está
    /// [`Wavetable::default_table`].
    pub fn default_sine() -> Self {
        let samples = (0..TABLE_RESOLUTION)
            .map(|index| {
                (index as f32 / TABLE_RESOLUTION as f32 * std::f32::consts::TAU).sin()
            })
            .collect();

        Self { name: "Sine".to_string(), path: None, samples, frames: 1 }
    }

    /// La tabla de fábrica: [`DEFAULT_CYCLES`] ciclos que van del casi-seno al
    /// diente de sierra.
    ///
    /// # Por qué no `DEFAULT_CYCLES` copias del seno
    ///
    /// Repetir un seno 256 veces daría una tabla con 256 ciclos, que es el
    /// número que pide el knob, y un visor inútil: las 256 formas serían
    /// idénticas, así que arrastrar el `WT POS` pasearía un realce por encima de
    /// copias de la misma curva sin que cambie nada. El número de ciclos es la
    /// mitad de lo que hace una wavetable; la otra mitad es que difieran.
    ///
    /// # Cómo se construye
    ///
    /// Síntesis aditiva: cada ciclo es la suma de sus armónicos, con la cantidad
    /// y la caída del espectro Slideendo con la posición en la tabla. El primer
    /// ciclo es un casi-seno ([`DARK_ROLL_OFF`]) y el último un diente de sierra
    /// denso ([`BRIGHT_ROLL_OFF`]), que es exactamente el recorrido que se ve al
    /// arrastrar el knob de abajo hacia arriba.
    ///
    /// Cada ciclo se normaliza a pico 1. Es lo que hace comparables los ciclos
    /// entre sí: la malla calcula su escala con el pico del frame, y sin
    /// normalizar un ciclo brillante dwarfs haría que los apagados se vieran
    /// planos al lado de él.
    ///
    /// # El costo
    ///
    /// Son 256 x 2048 x hasta 32 acumulaciones, unas 16 millones. Con `sin` en
    /// el ciclo interno serían 16 millones de transcendentes, varios cientos de ms
    /// en debug; por eso el seno va en una tabla y el avance de fase se lleva
    /// con un índice entero. Aun así es trabajo de arranque, no de cada frame, y
    /// sólo ocurre una vez por proceso.
    pub fn default_table() -> Self {
        let cycles = DEFAULT_CYCLES;
        let mut samples = Vec::with_capacity(cycles * FRAME_SAMPLES);

        // Seno de un período, precalculado: el índice avanza `harmonic` muestras
        // por muestra, que es exactamente `harmonic` vueltas por ciclo.
        let sine: Vec<f32> = (0..FRAME_SAMPLES)
            .map(|index| (index as f32 / FRAME_SAMPLES as f32 * std::f32::consts::TAU).sin())
            .collect();

        let mut frame = vec![0.0f32; FRAME_SAMPLES];
        for cycle in 0..cycles {
            let position = cycle as f32 / (cycles - 1).max(1) as f32;

            // Cuántos armónicos entran, y con qué caída.
            let highest = 1.0 + position * (MAX_HARMONICS as f32 - 1.0);
            let roll_off = DARK_ROLL_OFF + (BRIGHT_ROLL_OFF - DARK_ROLL_OFF) * position;

            for value in frame.iter_mut() {
                *value = 0.0;
            }

            for harmonic in 1..=MAX_HARMONICS {
                // El corte de armónicos se aplica **continuo**, no como un
                // entero.
                //
                // La primera versión usaba `if harmonic > highest { break }`, y
                // eso cuantizaba el corte: al principio de la tabla `highest` va
                // de 1.0 a 1.12 durante los primeros ciclos, y en los dos casos
                // el corte cae después del armónico 1. Los primeros ciclos de la
                // tabla salían con un solo armónico, todos iguales entre sí, y
                // el knob arrancaba en un tramo donde no se movía nada: el
                // error se veía justo en lo primero que el usuario arrastra.
                //
                // Con un peso gradual, el armónico que queda justo afuera entra
                // con la amplitud que le corresponde por su posición relative
                // al corte, y cada ciclo es distinto del anterior.
                let past_cutoff = (harmonic as f32 - highest).clamp(0.0, 1.0);
                let presence = 1.0 - past_cutoff;
                if presence <= 0.0 {
                    break;
                }

                // `1 / k^roll_off` da la caída del espectro, que es la forma
                // natural de una wavetable: el primer ciclo es casi un seno y el
                // último es un diente de sierra con todos los armónicos.
                let amplitude = presence / (harmonic as f32).powf(roll_off);

                // Fase con avance entero. `phase` siempre queda menor que
                // `FRAME_SAMPLES` al entrar, y `harmonic` es a lo sumo 32, así
                // que una sola resta alcanza y no hace falta un módulo.
                let mut phase = 0usize;
                for value in frame.iter_mut() {
                    *value += amplitude * sine[phase];
                    phase += harmonic;
                    if phase >= FRAME_SAMPLES {
                        phase -= FRAME_SAMPLES;
                    }
                }
            }

            // Pico a 1.0, para que todos los ciclos se vean al mismo tamaño.
            let peak = frame.iter().fold(0.0f32, |acc, v| acc.max(v.abs())).max(1.0e-6);
            for value in frame.iter_mut() {
                *value /= peak;
            }

            samples.extend_from_slice(&frame);
        }

        // DIAGNÓSTICO TEMPORAL: sacar cuando se encuentre la causa del
        // cruzido al cargar wavetables.
        eprintln!(
            "[DIAG] tabla de fábrica: samples={} frames={} nombre=Factory",
            samples.len(),
            cycles
        );

        Self {
            name: "Factory".to_string(),
            path: None,
            samples,
            frames: cycles,
        }
    }

    /// Un frame de la tabla, listo para dibujar o mandar a la malla.
    ///
    /// Devuelve siempre `TABLE_RESOLUTION` samples, estén o no en el archivo:
    /// la malla y el hash del render asumen ese largo, y un frame corto haría
    /// que la cinta se cerrara antes de tiempo.
    ///
    /// Con `smooth` interpola entre el frame pedido y el siguiente: es lo que
    /// hace que el knob de morph sea continuo en vez de dar saltos entre formas
    /// discretas.
    pub fn frame(&self, index: f32, smooth: bool) -> Vec<f32> {
        let last = (self.frames.saturating_sub(1)) as f32;
        let position = if index.is_finite() { index.clamp(0.0, last) } else { 0.0 };

        let base = position.floor() as usize;
        let next = (base + 1).min(self.frames - 1);
        let current = self.frame_samples(base);
        let following = self.frame_samples(next);

        if !smooth || self.frames <= 1 || base == next {
            return current.to_vec();
        }

        let fraction = position - base as f32;
        current
            .iter()
            .zip(following.iter())
            .map(|(a, b)| a + (b - a) * fraction)
            .collect()
    }

    /// Los samples del frame `index`, tal cual están.
    ///
    /// Con un archivo más corto que lo que el conteo de frames promete, el
    /// slice sale corto y [`Wavetable::frame`] lo rellena con el último sample
    /// en vez de dejar un hueco.
    fn frame_samples(&self, index: usize) -> &[f32] {
        if self.frames <= 1 {
            return &self.samples;
        }
        let start = index.min(self.frames - 1) * FRAME_SAMPLES;
        let end = (start + FRAME_SAMPLES).min(self.samples.len());
        self.samples.get(start..end).unwrap_or(&self.samples[..0])
    }

    /// Cuántos frames se pueden recorrer con el knob de índice.
    pub fn selectable_frames(&self) -> usize {
        self.frames.max(1)
    }
}

/// Errores al leer un archivo de wavetable.
#[derive(Debug)]
pub enum WavetableError {
    /// El archivo no existe o no se puede abrir.
    Open {
        /// Ruta que se intentó abrir.
        path: PathBuf,
        /// Motivo del sistema operativo o del decoder.
        source: String,
    },
    /// El archivo es un WAV pero no trae samples.
    NoSamples {
        /// Ruta del archivo.
        path: PathBuf,
    },
    /// Un canal del WAV vino con muestras no finitas o fuera de rango.
    ///
    /// No es un error fatal: se recorta. Está en el enum para que quien llame
    /// pueda avisarle al usuario, porque un `.wav` roto igual suena a ruido en
    /// cualquier lado.
    OutOfRange {
        /// Cuántas muestras hubo que recortar.
        clipped: usize,
    },
}

impl std::fmt::Display for WavetableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WavetableError::Open { path, source } => {
                write!(f, "no se pudo abrir {}: {source}", path.display())
            }
            WavetableError::NoSamples { path } => {
                write!(f, "{} no tiene muestras de audio", path.display())
            }
            WavetableError::OutOfRange { clipped } => {
                write!(f, "{clipped} muestras estaban fuera de -1..1 y se recortaron")
            }
        }
    }
}

impl std::error::Error for WavetableError {}

/// Extensiones que se ofrecen al elegir archivo.
///
/// Sólo WAV: los otros formatos (FLAC, AIFF) requieren otro decoder, y
/// `hound` no los trae. Preferirlo así es mejor que dejar que el diálogo de
/// archivos ofrezca algo que después no se puede cargar.
pub const WAVETABLE_EXTENSIONS: [&str; 1] = ["wav"];

/// El `.wav` de la wavetable inicial, embebido en el binario.
///
/// Va con `include_bytes!` y no como ruta en disco a propósito: el ejecutable
/// puede arrancar desde cualquier directorio, y una tabla cargada desde disco
/// depende de que el archivo siga ahí al lado del binario. Con los bytes dentro,
/// el slot inicial siempre tiene wavetable.
pub const BUNDLED_WAVETABLE: &[u8] = include_bytes!("../../../../assets/wavetables/factory.wav");

/// El nombre que muestra el slot inicial.
/// El nombre que muestra el slot inicial.
///
/// Sale del nombre del archivo embebido, no de una constante aparte: tener las
/// dos cosas por separado hacía que renombrar el `.wav` no cambiara lo que
/// muestra el slot, y el módulo seguía diciendo "Factory.wav" con un archivo
/// que se llamaba otra cosa.
pub const BUNDLED_WAVETABLE_NAME: &str = "Basic Shapes";

/// Carga la wavetable inicial desde los bytes embebidos.
///
/// Si el `.wav` embebido no se puede leer, `WavetableEditor::default` cae a la
/// tabla sintetizada: arrancar sin wavetable es peor que arrancar con una
/// distinta a la que se pidió.
pub fn bundled_wavetable(max_frames: usize) -> Result<Wavetable, WavetableError> {
    let reader = hound::WavReader::new(std::io::Cursor::new(BUNDLED_WAVETABLE))
        .map_err(|source| WavetableError::Open {
            path: std::path::PathBuf::from("assets/wavetables/factory.wav"),
            source: source.to_string(),
        })?;
    let table = decode_reader(reader, max_frames)?;
    Ok(Wavetable {
        name: BUNDLED_WAVETABLE_NAME.to_string(),
        // `None` a propósito: no viene de disco, así que no debe aparecer como
        // una de las wavetables hermanas de su directorio.
        path: None,
        ..table
    })
}

/// Carga una wavetable desde un `.wav`.
///
/// `max_frames` recorta archivos con muchos ciclos: un `.wav` de una pista
/// entera de dos minutos tiene 4 millones de samples y 2000 frames, y 2000
/// frames de 2048 son 16 MB en memoria por un solo slot del rack.
pub fn load_wavetable(path: &Path, max_frames: usize) -> Result<Wavetable, WavetableError> {
    let name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "Wavetable".to_string());

    let mut reader = hound::WavReader::open(path).map_err(|error| WavetableError::Open {
        path: path.to_path_buf(),
        source: error.to_string(),
    })?;

    let mut table = decode_reader(reader, max_frames)?;
    table.name = name;
    table.path = Some(path.to_path_buf());
    Ok(table)
}

/// Decodifica un `WavReader` ya abierto a una wavetable usable.
///
/// Va separado de [`load_wavetable`] para que el `.wav` embebido del binario
/// y el de disco compartan exactamente la misma decodificación: duplicar eso
/// hacía que el formato se pueda arreglar en un camino y no en el otro.
fn decode_reader<R: std::io::Read + std::io::Seek>(
    mut reader: hound::WavReader<R>,
    max_frames: usize,
) -> Result<Wavetable, WavetableError> {
    let embedded = std::path::PathBuf::from("assets/wavetables/factory.wav");
    let path = &embedded;

    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let total_frames = reader.len() as usize;
    if total_frames == 0 {
        return Err(WavetableError::NoSamples { path: path.to_path_buf() });
    }

    // Un archivo más largo que un frame se parte en frames. Con menos de uno,
    // se re-muestrea a TABLE_RESOLUTION: un ciclo de 400 samples tiene otro
    // "punto" en la cinta que uno de 2048.
    let frames = (total_frames / FRAME_SAMPLES).max(1).min(max_frames.max(1));
    let kept_frames = total_frames.min(frames * FRAME_SAMPLES);
    let mut mono = Vec::with_capacity(kept_frames);

    match spec.sample_format {
        hound::SampleFormat::Int => {
            let full_scale = (1i32 << (spec.bits_per_sample.saturating_sub(1)).min(30)) as f32;
            for sample in reader.samples::<i32>().take(kept_frames * channels) {
                if let Ok(value) = sample {
                    mono.push(value as f32 / full_scale);
                }
            }
        }
        hound::SampleFormat::Float => {
            for sample in reader.samples::<f32>().take(kept_frames * channels) {
                if let Ok(value) = sample {
                    mono.push(value);
                }
            }
        }
    }

    if mono.is_empty() {
        return Err(WavetableError::NoSamples { path: path.to_path_buf() });
    }

    // De multicanal a mono por promedio: una wavetable con dos canales idénticos
    // es lo normal, y el desfasaje entre canales de un archivo estéreo cualquiero
    // suena a coro.
    let mono = if channels > 1 { downmix(&mono, channels) } else { mono };

    // El destino es la tabla entera, no un frame: con varios ciclos hay que
    // guardarlos todos o el knob de índice no tiene nada que recorrer.
    let (samples, clipped) = normalize(&mono, kept_frames, FRAME_SAMPLES * frames);
    if clipped > 0 {
        eprintln!("[Hikaru] {}: {clipped} muestras fuera de rango, recortadas", path.display());
    }

    Ok(Wavetable { name: String::new(), path: None, samples, frames })
}

/// Promedia los canales de un buffer entrelazado.
fn downmix(samples: &[f32], channels: usize) -> Vec<f32> {
    samples.chunks_exact(channels).map(|frame| frame.iter().sum::<f32>() / channels as f32).collect()
}

/// Re-muestrea a `target_len` y recorta a -1..1.
///
/// `source_len` es la cantidad de samples que existen realmente, que puede ser
/// menor que la del archivo declarado: acá es donde se decide si sobra tabla o
/// si hay que estirar.
fn normalize(samples: &[f32], source_len: usize, target_len: usize) -> (Vec<f32>, usize) {
    if samples.is_empty() {
        return (vec![0.0; target_len], 0);
    }

    let available = source_len.min(samples.len()).max(1);
    let mut out = Vec::with_capacity(target_len);
    let mut clipped = 0;

    for index in 0..target_len {
        let position = index as f32 / target_len as f32 * available as f32;
        let base = position.floor() as usize % available;
        let next = (base + 1) % available;
        let fraction = position - position.floor();

        // El `next` envuelve al principio: sin eso, un archivo de una sola
        // vuelta de samples deja una costura en la cinta.
        let value = samples[base.min(samples.len() - 1)] * (1.0 - fraction)
            + samples[next.min(samples.len() - 1)] * fraction;

        // Un sample no finito arruina la normalización de la malla: la cinta
        // desaparece entera. Se lo trata como silencio.
        let value = if value.is_finite() { value } else { 0.0 };

        if value.abs() > 1.0 {
            clipped += 1;
        }
        out.push(value.clamp(-1.0, 1.0));
    }

    (out, clipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pico absoluto de un ciclo.
    fn peak(frame: &[f32]) -> f32 {
        frame.iter().fold(0.0f32, |acc, v| acc.max(v.abs()))
    }

    /// El frame `index` de la tabla.
    fn frame_of(table: &Wavetable, index: usize) -> &[f32] {
        let start = index * FRAME_SAMPLES;
        &table.samples[start..start + FRAME_SAMPLES]
    }

    /// Distancia RMS entre dos ciclos.
    fn rms_difference(a: &[f32], b: &[f32]) -> f32 {
        let mut sum = 0.0f32;
        for (x, y) in a.iter().zip(b.iter()) {
            let d = x - y;
            sum += d * d;
        }
        (sum / a.len() as f32).sqrt()
    }

    #[test]
    fn the_factory_table_has_the_declared_number_of_cycles() {
        let table = Wavetable::default_table();
        assert_eq!(table.frames, DEFAULT_CYCLES);
        assert_eq!(table.samples.len(), DEFAULT_CYCLES * FRAME_SAMPLES);
        assert_eq!(table.selectable_frames(), DEFAULT_CYCLES);
    }

    #[test]
    fn every_cycle_is_peak_normalized_and_finite() {
        // La malla escala cada cinta con el pico de su propio frame. Si un
        // ciclo no llegara a 1.0 se vería más chico que los demás, y comparar
        // timbres dejaría de ser sobre la forma.
        let table = Wavetable::default_table();
        for index in 0..table.frames {
            let frame = frame_of(&table, index);
            assert!(frame.iter().all(|v| v.is_finite()), "el ciclo {index} tiene NaN");
            assert!(
                (peak(frame) - 1.0).abs() < 1.0e-3,
                "el ciclo {index} tiene pico {} y debería tener 1.0",
                peak(frame)
            );
        }
    }

    #[test]
    fn the_cycles_actually_differ() {
        // El test que justifica toda la tabla: si los 256 ciclos fuesen el mismo
        // seno, el knob `WT POS` recorrería 256 copias de la misma curva y la
        // matriz no diría nada.
        let table = Wavetable::default_table();

        let first = rms_difference(frame_of(&table, 0), frame_of(&table, table.frames - 1));
        assert!(first > 0.3, "el primer y el último ciclo son casi iguales: RMS {first}");

        // Y los vecinos también: si no, el barrido del knob se ve como escalones.
        for index in 0..table.frames - 1 {
            let step = rms_difference(frame_of(&table, index), frame_of(&table, index + 1));
            assert!(
                step > 1.0e-4,
                "los ciclos {index} y {} son idénticos: RMS {step}",
                index + 1
            );
        }
    }

    #[test]
    fn every_cycle_is_continuous_at_its_own_boundary() {
        // Un oscilador de tabla necesita que el último sample del ciclo pegue con
        // el primero, y en el visor 3D además se ve como una costura en el lazo
        // de la cinta.
        //
        // La costura **no** tiene que ser cero, y ese fue el error de la primera
        // versión de este test: un diente de sierra baja a tope justo antes de
        // cerrar el ciclo, así que el salto del último sample al primero es
        // la pendiente de la sierra, no un click. Lo que no puede pasar es que
        // ese salto sea mayor que cualquier otro del ciclo: eso sí sería un
        // corte, porque la forma vuelve al principio con un escalón.
        let table = Wavetable::default_table();
        for index in 0..table.frames {
            let frame = frame_of(&table, index);

            let seam = (frame[FRAME_SAMPLES - 1] - frame[0]).abs();
            let steepest = frame
                .windows(2)
                .map(|pair| (pair[1] - pair[0]).abs())
                .fold(0.0f32, f32::max);

            assert!(
                seam <= steepest * 1.5 + 1.0e-4,
                "el ciclo {index} tiene una costura de {seam} y su pendiente \
                 normal más fuerte es {steepest}: es un corte, no una pendiente"
            );
        }
    }

    #[test]
    fn default_sine_stays_a_single_cycle() {
        // El seno de un ciclo es una de las entradas del menú, y por definición
        // no tiene recorrido: el knob tiene que quedar apagado con ella.
        let table = Wavetable::default_sine();
        assert_eq!(table.frames, 1);
        assert_eq!(table.samples.len(), TABLE_RESOLUTION);
    }
}

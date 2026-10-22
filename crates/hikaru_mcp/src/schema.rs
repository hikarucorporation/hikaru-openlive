// crates/hikaru_mcp/src/schema.rs
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Ayudantes para armar los JSON Schema de las herramientas.
//!
//! El SDK de MCP (`rmcp`) deja el schema como un `serde_json::Map` libre:
//! el servidor decide qué validar. Acá hay los atajos para no repetir el
//! `json!` a mano en cada herramienta.

use serde_json::{json, Map, Value};

/// El tipo de un parámetro, como lo entiende JSON Schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Texto libre.
    String,
    /// Entero.
    Integer,
    /// Número con decimales.
    Number,
    /// Verdadero o falso.
    Boolean,
}

impl Kind {
    /// El nombre del tipo en JSON Schema.
    pub fn type_name(self) -> &'static str {
        match self {
            Kind::String => "string",
            Kind::Integer => "integer",
            Kind::Number => "number",
            Kind::Boolean => "boolean",
        }
    }
}

/// Un parámetro de una herramienta.
#[derive(Debug, Clone)]
pub struct Param {
    /// El nombre del argumento, tal como lo ve el agente.
    pub name: &'static str,
    /// Qué es.
    pub description: &'static str,
    /// De qué tipo.
    pub kind: Kind,
    /// Si el agente tiene que mandarlo sí o sí.
    pub required: bool,
    /// Un valor por omisión, si lo hay.
    pub default: Option<Value>,
    /// Los valores permitidos, si el parámetro es de choices.
    pub enum_values: Option<&'static [&'static str]>,
}

impl Param {
    /// Un parámetro de texto obligatorio.
    pub fn string(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            kind: Kind::String,
            required: true,
            default: None,
            enum_values: None,
        }
    }

    /// Un parámetro de texto opcional.
    pub fn opt_string(name: &'static str, description: &'static str) -> Self {
        Self {
            required: false,
            ..Self::string(name, description)
        }
    }

    /// Un parátero entero obligatorio.
    pub fn int(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            kind: Kind::Integer,
            required: true,
            default: None,
            enum_values: None,
        }
    }

    /// Un parámetro entero opcional.
    pub fn opt_int(name: &'static str, description: &'static str) -> Self {
        Self {
            required: false,
            ..Self::int(name, description)
        }
    }

    /// Un parámetro decimal obligatorio.
    pub fn number(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            kind: Kind::Number,
            required: true,
            default: None,
            enum_values: None,
        }
    }

    /// Un parámetro decimal opcional.
    pub fn opt_number(name: &'static str, description: &'static str) -> Self {
        Self {
            required: false,
            ..Self::number(name, description)
        }
    }

    /// Un parámetro booleano opcional.
    pub fn opt_bool(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            kind: Kind::Boolean,
            required: false,
            default: None,
            enum_values: None,
        }
    }

    /// Marca el parámetro como opcional y le pone un valor por omisión.
    pub fn with_default(mut self, value: Value) -> Self {
        self.required = false;
        self.default = Some(value);
        self
    }

    /// Limita los valores permitidos.
    pub fn with_choices(mut self, values: &'static [&'static str]) -> Self {
        self.enum_values = Some(values);
        self
    }
}

/// Arma el JSON Schema de una herramienta a partir de sus parámetros.
///
/// El schema que sale es un objeto con `type: "object"`, `properties` y
/// `required`, que es lo que el protocolo espera en `inputSchema`.
pub fn object_schema(params: &[Param]) -> Map<String, Value> {
    let mut properties = Map::new();
    let mut required = Vec::new();

    for param in params {
        let mut schema = Map::new();
        schema.insert("type".to_string(), json!(param.kind.type_name()));
        schema.insert(
            "description".to_string(),
            json!(param.description),
        );

        if let Some(values) = param.enum_values {
            schema.insert(
                "enum".to_string(),
                Value::Array(values.iter().map(|v| json!(v)).collect()),
            );
        }
        if let Some(default) = &param.default {
            schema.insert("default".to_string(), default.clone());
        }

        properties.insert(param.name.to_string(), Value::Object(schema));

        if param.required {
            required.push(Value::String(param.name.to_string()));
        }
    }

    let mut root = Map::new();
    root.insert("type".to_string(), json!("object"));
    root.insert("properties".to_string(), Value::Object(properties));
    if !required.is_empty() {
        root.insert("required".to_string(), Value::Array(required));
    }
    root
}

/// Un schema de objeto sin propiedades: para las herramientas que no toman
/// argumentos.
pub fn empty_schema() -> Map<String, Value> {
    let mut root = Map::new();
    root.insert("type".to_string(), json!("object"));
    root.insert("properties".to_string(), json!({}));
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_schema_declara_los_requeridos() {
        let schema = object_schema(&[
            Param::string("path", "ruta"),
            Param::opt_int("indice", "indice"),
        ]);

        assert_eq!(schema.get("type").unwrap(), "object");
        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert_eq!(props.len(), 2);
        assert_eq!(props["path"]["type"], json!("string"));
        assert_eq!(props["indice"]["type"], json!("integer"));

        let requeridos = schema.get("required").unwrap().as_array().unwrap();
        assert_eq!(requeridos.len(), 1);
        assert_eq!(requeridos[0], json!("path"));
    }

    #[test]
    fn un_parametro_con_default_deja_de_ser_requerido() {
        let schema = object_schema(&[
            Param::string("path", "ruta").with_default(json!("x")),
        ]);
        assert!(schema.get("required").is_none());
        assert_eq!(schema["properties"]["path"]["default"], json!("x"));
    }

    #[test]
    fn los_choices_limitan_los_valores() {
        let schema = object_schema(&[Param::opt_string(
            "modo",
            "modo",
        )
        .with_choices(&["live", "files"])]);
        assert_eq!(schema["properties"]["modo"]["enum"], json!(["live", "files"]));
    }
}

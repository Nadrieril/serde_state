use syn::{Attribute, Ident, LitStr};

pub struct FieldAttrs {
    pub rename: Option<String>,
    pub skip: bool,
}

impl FieldAttrs {
    pub fn new() -> Self {
        FieldAttrs {
            rename: None,
            skip: false,
        }
    }

    pub fn key(&self, ident: &Ident) -> String {
        self.rename.clone().unwrap_or_else(|| ident.to_string())
    }
}

pub fn parse_field_attrs(attrs: &[Attribute]) -> FieldAttrs {
    let mut result = FieldAttrs::new();
    for attr in attrs {
        if attr.path().is_ident("serde") {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    let value: LitStr = meta.value()?.parse()?;
                    result.rename = Some(value.value());
                    return Ok(());
                }
                if meta.path.is_ident("skip") {
                    result.skip = true;
                    return Ok(());
                }
                Ok(())
            });
        }
    }
    result
}

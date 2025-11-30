use syn::Attribute;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ItemMode {
    #[default]
    Stateful,
    Stateless,
}

pub fn merge_modes(default: ItemMode, override_mode: Option<ItemMode>) -> ItemMode {
    override_mode.unwrap_or(default)
}

pub fn attrs_mode(attrs: &[Attribute]) -> Option<ItemMode> {
    let mut mode = None;
    for attr in attrs {
        if attr.path().is_ident("serde_state") {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("stateless") {
                    mode = Some(ItemMode::Stateless);
                    return Ok(());
                }
                if meta.path.is_ident("stateful") {
                    mode = Some(ItemMode::Stateful);
                    return Ok(());
                }
                Ok(())
            });
        }
    }
    mode
}

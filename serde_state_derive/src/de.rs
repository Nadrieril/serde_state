use crate::dummy;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{
    parse_quote, Attribute, Data, DataStruct, DeriveInput, Fields, FieldsNamed, GenericParam,
    Generics,
};

pub fn expand_derive_deserialize(input: &DeriveInput) -> syn::Result<TokenStream> {
    let attrs = ContainerAttributes::from_attrs(&input.attrs)?;
    let impl_block = match &input.data {
        Data::Struct(data) => derive_struct(input, data, &attrs)?,
        Data::Enum(e) => {
            return Err(syn::Error::new(
                e.enum_token.span(),
                "DeserializeState currently only supports structs with named fields",
            ));
        }
        Data::Union(u) => {
            return Err(syn::Error::new(
                u.union_token.span(),
                "DeserializeState does not support unions",
            ));
        }
    };

    Ok(dummy::wrap_in_const(attrs.serde_path.as_ref(), impl_block))
}

fn derive_struct(
    input: &DeriveInput,
    data: &DataStruct,
    attrs: &ContainerAttributes,
) -> syn::Result<TokenStream> {
    let named = match &data.fields {
        Fields::Named(named) => named,
        other => {
            return Err(syn::Error::new(
                other.span(),
                "DeserializeState currently only supports structs with named fields",
            ));
        }
    };

    let (impl_generics, ty_generics, where_clause, state_tokens) = match &attrs.state_type {
        Some(state_ty) => {
            let generics_with_lifetime = add_de_lifetime(&input.generics);
            let (impl_generics_ref, _, _) = generics_with_lifetime.split_for_impl();
            let impl_generics = quote!(#impl_generics_ref);
            let (_, ty_generics_ref, _) = input.generics.split_for_impl();
            let ty_generics = quote!(#ty_generics_ref);
            let mut where_clause = input.generics.where_clause.clone();
            let state_tokens = quote!(#state_ty);
            add_deserialize_bounds(&mut where_clause, named, &state_tokens);
            (impl_generics, ty_generics, where_clause, state_tokens)
        }
        None => {
            let impl_generics_with_state = add_state_param(&input.generics);
            let (impl_generics_ref, _, _) = impl_generics_with_state.split_for_impl();
            let impl_generics = quote!(#impl_generics_ref);
            let (_, ty_generics_ref, _) = input.generics.split_for_impl();
            let ty_generics = quote!(#ty_generics_ref);
            let mut where_clause = input.generics.where_clause.clone();
            let state_tokens = quote!(__State);
            add_deserialize_bounds(&mut where_clause, named, &state_tokens);
            (impl_generics, ty_generics, where_clause, state_tokens)
        }
    };
    let where_clause_tokens = match &where_clause {
        Some(clause) => quote!(#clause),
        None => TokenStream::new(),
    };
    let uses_generic_state = attrs.state_type.is_none();
    let ident = &input.ident;

    let body = if attrs.transparent {
        deserialize_transparent(ident, named, &state_tokens)?
    } else {
        deserialize_fields(ident, named, &state_tokens, uses_generic_state)
    };

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics _serde_state::DeserializeState<'de, #state_tokens> for #ident #ty_generics #where_clause_tokens {
            fn deserialize_state<__D>(
                __state: &#state_tokens,
                __deserializer: __D,
            ) -> ::core::result::Result<Self, __D::Error>
            where
                __D: _serde::Deserializer<'de>,
            {
                #body
            }
        }
    })
}

fn deserialize_transparent(
    ident: &syn::Ident,
    fields: &FieldsNamed,
    state_tokens: &TokenStream,
) -> syn::Result<TokenStream> {
    if fields.named.len() != 1 {
        return Err(syn::Error::new(
            fields.span(),
            "transparent structs must have exactly one field",
        ));
    }
    let field = fields.named.first().unwrap();
    let field_ident = field.ident.as_ref().unwrap();
    let ty = &field.ty;
    Ok(quote! {
        let __seed = _serde_state::__private::wrap_deserialize_seed::<#ty, #state_tokens>(__state);
        let #field_ident = _serde::de::DeserializeSeed::deserialize(__seed, __deserializer)?;
        Ok(#ident { #field_ident: #field_ident })
    })
}

fn deserialize_fields(
    ident: &syn::Ident,
    fields: &FieldsNamed,
    state_tokens: &TokenStream,
    uses_generic_state: bool,
) -> TokenStream {
    let field_names: Vec<String> = fields
        .named
        .iter()
        .map(|field| field.ident.as_ref().unwrap().to_string())
        .collect();

    let field_variants: Vec<_> = field_names
        .iter()
        .map(|name| format_ident!("__field_{}", name))
        .collect();

    let const_fields = {
        let names = field_names.iter();
        quote! {
            const __FIELDS: &'static [&'static str] = &[#(#names),*];
        }
    };

    let field_enum = {
        let variants = field_variants.iter();
        quote! {
            #[allow(non_camel_case_types)]
            enum __Field { #(#variants,)* __Ignore }
        }
    };

    let field_visitor = {
        let match_arms = field_names
            .iter()
            .zip(field_variants.iter())
            .map(|(name, variant)| {
                quote! { #name => ::core::result::Result::Ok(__Field::#variant) }
            });
        quote! {
            struct __FieldVisitor;
            impl<'de> _serde::de::Visitor<'de> for __FieldVisitor {
                type Value = __Field;

                fn expecting(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    formatter.write_str("field name")
                }

                fn visit_str<E>(self, value: &str) -> ::core::result::Result<Self::Value, E>
                where
                    E: _serde::de::Error,
                {
                    match value {
                        #(#match_arms,)*
                        _ => ::core::result::Result::Ok(__Field::__Ignore),
                    }
                }
            }

            impl<'de> _serde::Deserialize<'de> for __Field {
                fn deserialize<D>(deserializer: D) -> ::core::result::Result<Self, D::Error>
                where
                    D: _serde::Deserializer<'de>,
                {
                    deserializer.deserialize_identifier(__FieldVisitor)
                }
            }
        }
    };

    let init_locals = fields.named.iter().map(|field| {
        let ident = field.ident.as_ref().unwrap();
        quote!(let mut #ident = ::core::option::Option::None;)
    });

    let match_arms = fields
        .named
        .iter()
        .zip(field_variants.iter())
        .map(|(field, variant)| {
        let ident = field.ident.as_ref().unwrap();
        let name = ident.to_string();
        let ty = &field.ty;
        quote! {
            __Field::#variant => {
                if #ident.is_some() {
                    return ::core::result::Result::Err(_serde::de::Error::duplicate_field(#name));
                }
                let __seed = _serde_state::__private::wrap_deserialize_seed::<#ty, #state_tokens>(state);
                #ident = ::core::option::Option::Some(
                    _serde::de::MapAccess::next_value_seed(&mut __map, __seed)?,
                );
            }
        }
    });

    let build_fields = fields.named.iter().map(|field| {
        let ident = field.ident.as_ref().unwrap();
        let name = ident.to_string();
        quote! {
            let #ident = match #ident {
                ::core::option::Option::Some(value) => value,
                ::core::option::Option::None =>
                    return ::core::result::Result::Err(_serde::de::Error::missing_field(#name)),
            };
        }
    });

    let construct = {
        let pairs = fields.named.iter().map(|field| {
            let ident = field.ident.as_ref().unwrap();
            quote!(#ident: #ident)
        });
        quote!(#ident { #(#pairs),* })
    };

    let visitor_struct = if uses_generic_state {
        quote! {
            struct __Visitor<'state, __State: ?Sized> {
                state: &'state __State,
            }
        }
    } else {
        quote! {
            struct __Visitor<'state> {
                state: &'state #state_tokens,
            }
        }
    };

    let visitor_impl = if uses_generic_state {
        quote! {
            impl<'de, 'state, __State: ?Sized> _serde::de::Visitor<'de> for __Visitor<'state, __State> {
                type Value = #ident;

                fn expecting(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    formatter.write_str("struct ")?;
                    formatter.write_str(stringify!(#ident))
                }

                fn visit_map<__M>(self, mut __map: __M) -> ::core::result::Result<Self::Value, __M::Error>
                where
                    __M: _serde::de::MapAccess<'de>,
                {
                    let state = self.state;
                    #(#init_locals)*
                    while let ::core::option::Option::Some(key) =
                        _serde::de::MapAccess::next_key::<__Field>(&mut __map)?
                    {
                        match key {
                            #(#match_arms)*
                            __Field::__Ignore => {
                                let _ = _serde::de::MapAccess::next_value::<_serde::de::IgnoredAny>(&mut __map)?;
                            }
                        }
                    }
                    #(#build_fields)*
                    ::core::result::Result::Ok(#construct)
                }
            }
        }
    } else {
        quote! {
            impl<'de, 'state> _serde::de::Visitor<'de> for __Visitor<'state> {
                type Value = #ident;

                fn expecting(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    formatter.write_str("struct ")?;
                    formatter.write_str(stringify!(#ident))
                }

                fn visit_map<__M>(self, mut __map: __M) -> ::core::result::Result<Self::Value, __M::Error>
                where
                    __M: _serde::de::MapAccess<'de>,
                {
                    let state = self.state;
                    #(#init_locals)*
                    while let ::core::option::Option::Some(key) =
                        _serde::de::MapAccess::next_key::<__Field>(&mut __map)?
                    {
                        match key {
                            #(#match_arms)*
                            __Field::__Ignore => {
                                let _ = _serde::de::MapAccess::next_value::<_serde::de::IgnoredAny>(&mut __map)?;
                            }
                        }
                    }
                    #(#build_fields)*
                    ::core::result::Result::Ok(#construct)
                }
            }
        }
    };

    quote! {
        #const_fields
        #field_enum
        #field_visitor

        #visitor_struct

        #visitor_impl

        _serde::Deserializer::deserialize_struct(
            __deserializer,
            stringify!(#ident),
            __FIELDS,
            __Visitor { state: __state },
        )
    }
}

fn add_state_param(generics: &Generics) -> Generics {
    let mut generics = generics.clone();
    let lifetime: syn::LifetimeParam = parse_quote!('de);
    generics.params.insert(0, GenericParam::Lifetime(lifetime));
    generics.params.push(parse_quote!(__State: ?Sized));
    generics
}

fn add_de_lifetime(generics: &Generics) -> Generics {
    let mut generics = generics.clone();
    let lifetime: syn::LifetimeParam = parse_quote!('de);
    generics.params.insert(0, GenericParam::Lifetime(lifetime));
    generics
}

fn add_deserialize_bounds(
    where_clause: &mut Option<syn::WhereClause>,
    fields: &FieldsNamed,
    state_ty: &TokenStream,
) {
    if fields.named.is_empty() {
        return;
    }

    let clause = where_clause.get_or_insert_with(|| syn::WhereClause {
        where_token: Default::default(),
        predicates: Default::default(),
    });

    for field in &fields.named {
        let ty = &field.ty;
        clause
            .predicates
            .push(parse_quote!(#ty: _serde_state::DeserializeState<'de, #state_ty>));
    }
}

struct ContainerAttributes {
    transparent: bool,
    serde_path: Option<syn::Path>,
    state_type: Option<syn::Type>,
}

impl ContainerAttributes {
    fn from_attrs(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut result = ContainerAttributes {
            transparent: false,
            serde_path: None,
            state_type: None,
        };

        for attr in attrs {
            if !(attr.path().is_ident("serde") || attr.path().is_ident("serde_state")) {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("transparent") {
                    result.transparent = true;
                    return Ok(());
                }
                if meta.path.is_ident("crate") {
                    let path = meta.value()?.parse()?;
                    result.serde_path = Some(path);
                    return Ok(());
                }
                if meta.path.is_ident("state") {
                    let ty = meta.value()?.parse()?;
                    result.state_type = Some(ty);
                    return Ok(());
                }
                Err(meta.error("unsupported serde attribute"))
            })?;
        }

        Ok(result)
    }
}

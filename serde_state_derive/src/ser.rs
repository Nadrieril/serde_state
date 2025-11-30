use crate::dummy;
use proc_macro2::TokenStream;
use quote::quote;
use syn::spanned::Spanned;
use syn::{parse_quote, Attribute, Data, DataStruct, DeriveInput, Fields, FieldsNamed, Generics};

pub fn expand_derive_serialize(input: &DeriveInput) -> syn::Result<TokenStream> {
    let attrs = ContainerAttributes::from_attrs(&input.attrs)?;
    let impl_block = match &input.data {
        Data::Struct(data) => derive_struct(input, data, &attrs)?,
        Data::Enum(e) => {
            return Err(syn::Error::new(
                e.enum_token.span(),
                "SerializeState currently only supports structs with named fields",
            ));
        }
        Data::Union(u) => {
            return Err(syn::Error::new(
                u.union_token.span(),
                "SerializeState does not support unions",
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
                "SerializeState currently only supports structs with named fields",
            ));
        }
    };

    let (impl_generics, ty_generics, where_clause, state_tokens) = match &attrs.state_type {
        Some(state_ty) => {
            let (impl_generics_ref, ty_generics_ref, _) = input.generics.split_for_impl();
            let impl_generics = quote!(#impl_generics_ref);
            let ty_generics = quote!(#ty_generics_ref);
            let mut where_clause = input.generics.where_clause.clone();
            let state_tokens = quote!(#state_ty);
            add_serialize_bounds(&mut where_clause, named, &state_tokens);
            (impl_generics, ty_generics, where_clause, state_tokens)
        }
        None => {
            let impl_generics_storage = add_state_param(&input.generics);
            let (impl_generics_ref, _, _) = impl_generics_storage.split_for_impl();
            let impl_generics = quote!(#impl_generics_ref);
            let (_, ty_generics_ref, _) = input.generics.split_for_impl();
            let ty_generics = quote!(#ty_generics_ref);
            let mut where_clause = input.generics.where_clause.clone();
            let state_tokens = quote!(__State);
            add_serialize_bounds(&mut where_clause, named, &state_tokens);
            (impl_generics, ty_generics, where_clause, state_tokens)
        }
    };
    let where_clause_tokens = match &where_clause {
        Some(clause) => quote!(#clause),
        None => TokenStream::new(),
    };
    let ident = &input.ident;

    let body = if attrs.transparent {
        serialize_transparent(named)?
    } else {
        serialize_fields(ident, named)
    };

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics _serde_state::SerializeState<#state_tokens> for #ident #ty_generics #where_clause_tokens {
            fn serialize_state<__S>(
                &self,
                __state: &#state_tokens,
                __serializer: __S,
            ) -> ::core::result::Result<__S::Ok, __S::Error>
            where
                __S: _serde::Serializer,
            {
                #body
            }
        }
    })
}

fn serialize_transparent(fields: &FieldsNamed) -> syn::Result<TokenStream> {
    if fields.named.len() != 1 {
        return Err(syn::Error::new(
            fields.span(),
            "transparent structs must have exactly one field",
        ));
    }
    let field = fields.named.first().unwrap().ident.as_ref().unwrap();
    Ok(quote! {
        _serde_state::SerializeState::serialize_state(&self.#field, __state, __serializer)
    })
}

fn serialize_fields(ident: &syn::Ident, fields: &FieldsNamed) -> TokenStream {
    let type_name = ident.to_string();
    let len = fields.named.len();
    let serialize_fields = fields.named.iter().map(|field| {
        let field_ident = field.ident.as_ref().unwrap();
        let key = field_ident.to_string();
        quote! {
            _serde::ser::SerializeStruct::serialize_field(
                &mut __serde_state,
                #key,
                &_serde_state::__private::wrap_serialize(&self.#field_ident, __state),
            )?;
        }
    });

    quote! {
        let mut __serde_state = _serde::Serializer::serialize_struct(__serializer, #type_name, #len)?;
        #(#serialize_fields)*
        _serde::ser::SerializeStruct::end(__serde_state)
    }
}

fn add_state_param(generics: &Generics) -> Generics {
    let mut generics = generics.clone();
    generics.params.push(parse_quote!(__State: ?Sized));
    generics
}

fn add_serialize_bounds(
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
            .push(parse_quote!(#ty: _serde_state::SerializeState<#state_ty>));
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

use crate::dummy;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{
    parse_quote, Attribute, Data, DataEnum, DataStruct, DeriveInput, Fields, FieldsNamed,
    FieldsUnnamed, GenericParam, Generics,
};

pub fn expand_derive_deserialize(input: &DeriveInput) -> syn::Result<TokenStream> {
    let attrs = ContainerAttributes::from_attrs(&input.attrs)?;
    let impl_block = match &input.data {
        Data::Struct(data) => derive_struct(input, data, &attrs)?,
        Data::Enum(data) => derive_enum(input, data, &attrs)?,
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
    let impl_generics_with_state = add_state_param(&input.generics);
    let (impl_generics_ref, _, _) = impl_generics_with_state.split_for_impl();
    let impl_generics = quote!(#impl_generics_ref);
    let (_, ty_generics_ref, _) = input.generics.split_for_impl();
    let ty_generics = quote!(#ty_generics_ref);
    let mut where_clause = input.generics.where_clause.clone();
    let state_tokens = quote!(__State);
    let field_types = collect_field_types_from_fields(&data.fields);
    add_deserialize_bounds_from_types(&mut where_clause, &field_types, &state_tokens);
    let where_clause_tokens = quote_where_clause(&where_clause);
    let ident = &input.ident;

    let body = if attrs.transparent {
        deserialize_transparent(ident, &data.fields, &state_tokens)?
    } else {
        deserialize_struct_body(ident, &data.fields, &state_tokens, &where_clause)
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

fn derive_enum(
    input: &DeriveInput,
    data: &DataEnum,
    _attrs: &ContainerAttributes,
) -> syn::Result<TokenStream> {
    let impl_generics_with_state = add_state_param(&input.generics);
    let (impl_generics_ref, _, _) = impl_generics_with_state.split_for_impl();
    let impl_generics = quote!(#impl_generics_ref);
    let (_, ty_generics_ref, _) = input.generics.split_for_impl();
    let ty_generics = quote!(#ty_generics_ref);
    let mut where_clause = input.generics.where_clause.clone();
    let state_tokens = quote!(__State);
    let field_types = collect_field_types_from_enum(data);
    add_deserialize_bounds_from_types(&mut where_clause, &field_types, &state_tokens);
    let where_clause_tokens = quote_where_clause(&where_clause);
    let ident = &input.ident;

    let body = deserialize_enum_body(ident, data, &state_tokens, &where_clause);

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
    fields: &Fields,
    state_tokens: &TokenStream,
) -> syn::Result<TokenStream> {
    match fields {
        Fields::Named(named) if named.named.len() == 1 => {
            let field = named.named.first().unwrap();
            let field_ident = field.ident.as_ref().unwrap();
            let ty = &field.ty;
            Ok(quote! {
                let __seed = _serde_state::__private::wrap_deserialize_seed::<#ty, #state_tokens>(__state);
                let #field_ident = _serde::de::DeserializeSeed::deserialize(__seed, __deserializer)?;
                ::core::result::Result::Ok(#ident { #field_ident: #field_ident })
            })
        }
        Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => {
            let ty = &unnamed.unnamed.first().unwrap().ty;
            Ok(quote! {
                let __seed = _serde_state::__private::wrap_deserialize_seed::<#ty, #state_tokens>(__state);
                let __value = _serde::de::DeserializeSeed::deserialize(__seed, __deserializer)?;
                ::core::result::Result::Ok(#ident(__value))
            })
        }
        other => Err(syn::Error::new(
            other.span(),
            "transparent structs must have exactly one field",
        )),
    }
}

fn deserialize_struct_body(
    ident: &syn::Ident,
    fields: &Fields,
    state_tokens: &TokenStream,
    where_clause: &Option<syn::WhereClause>,
) -> TokenStream {
    match fields {
        Fields::Named(named) => deserialize_named_struct(ident, named, state_tokens, where_clause),
        Fields::Unnamed(unnamed) => {
            deserialize_unnamed_struct(ident, unnamed, state_tokens, where_clause)
        }
        Fields::Unit => deserialize_unit_struct(ident),
    }
}

fn deserialize_named_struct(
    ident: &syn::Ident,
    fields: &FieldsNamed,
    state_tokens: &TokenStream,
    where_clause: &Option<syn::WhereClause>,
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

    let visitor_struct = quote! {
        struct __Visitor<'state, __State: ?Sized> {
            state: &'state __State,
        }
    };

    let visitor_where_clause = quote_where_clause(where_clause);
    let visitor_impl = quote! {
        impl<'de, 'state, __State: ?Sized> _serde::de::Visitor<'de> for __Visitor<'state, __State> #visitor_where_clause {
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

fn deserialize_unnamed_struct(
    ident: &syn::Ident,
    fields: &FieldsUnnamed,
    state_tokens: &TokenStream,
    where_clause: &Option<syn::WhereClause>,
) -> TokenStream {
    match fields.unnamed.len() {
        0 => deserialize_unit_struct(ident),
        1 => {
            let ty = &fields.unnamed.first().unwrap().ty;
            deserialize_newtype_struct(ident, ty, state_tokens, where_clause)
        }
        _ => deserialize_tuple_struct(ident, fields, state_tokens, where_clause),
    }
}

fn deserialize_newtype_struct(
    ident: &syn::Ident,
    field_ty: &syn::Type,
    state_tokens: &TokenStream,
    where_clause: &Option<syn::WhereClause>,
) -> TokenStream {
    let visitor_struct = quote! {
        struct __Visitor<'state, __State: ?Sized> {
            state: &'state __State,
        }
    };

    let visit_body = quote! {
        fn visit_newtype_struct<__E>(
            self,
            __deserializer: __E,
        ) -> ::core::result::Result<Self::Value, __E::Error>
        where
            __E: _serde::Deserializer<'de>,
        {
            let state = self.state;
            let __seed = _serde_state::__private::wrap_deserialize_seed::<#field_ty, #state_tokens>(state);
            let __value = _serde::de::DeserializeSeed::deserialize(__seed, __deserializer)?;
            ::core::result::Result::Ok(#ident(__value))
        }

        fn visit_seq<__A>(
            self,
            mut __seq: __A,
        ) -> ::core::result::Result<Self::Value, __A::Error>
        where
            __A: _serde::de::SeqAccess<'de>,
        {
            let state = self.state;
            let __seed = _serde_state::__private::wrap_deserialize_seed::<#field_ty, #state_tokens>(state);
            let __value = match _serde::de::SeqAccess::next_element_seed(&mut __seq, __seed)? {
                ::core::option::Option::Some(value) => value,
                ::core::option::Option::None =>
                    return ::core::result::Result::Err(_serde::de::Error::invalid_length(0, &self)),
            };
            if _serde::de::SeqAccess::next_element::<_serde::de::IgnoredAny>(&mut __seq)?.is_some() {
                return ::core::result::Result::Err(_serde::de::Error::invalid_length(1, &self));
            }
            ::core::result::Result::Ok(#ident(__value))
        }
    };

    let visitor_where_clause = quote_where_clause(where_clause);
    let visitor_impl = quote! {
        impl<'de, 'state, __State: ?Sized> _serde::de::Visitor<'de> for __Visitor<'state, __State> #visitor_where_clause {
            type Value = #ident;

            fn expecting(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                formatter.write_str("newtype struct ")?;
                formatter.write_str(stringify!(#ident))
            }

            #visit_body
        }
    };

    quote! {
        #visitor_struct
        #visitor_impl

        _serde::Deserializer::deserialize_newtype_struct(
            __deserializer,
            stringify!(#ident),
            __Visitor { state: __state },
        )
    }
}

fn deserialize_tuple_struct(
    ident: &syn::Ident,
    fields: &FieldsUnnamed,
    state_tokens: &TokenStream,
    where_clause: &Option<syn::WhereClause>,
) -> TokenStream {
    let len = fields.unnamed.len();
    let bindings: Vec<_> = (0..len).map(|i| format_ident!("__field_{}", i)).collect();
    let read_fields = fields.unnamed.iter().enumerate().map(|(index, field)| {
        let binding = &bindings[index];
        let ty = &field.ty;
        let idx = index;
        quote! {
            let #binding = match _serde::de::SeqAccess::next_element_seed(
                &mut __seq,
                _serde_state::__private::wrap_deserialize_seed::<#ty, #state_tokens>(state),
            )? {
                ::core::option::Option::Some(value) => value,
                ::core::option::Option::None =>
                    return ::core::result::Result::Err(_serde::de::Error::invalid_length(#idx, &self)),
            };
        }
    });

    let construct = quote!(#ident(#(#bindings),*));

    let visitor_struct = quote! {
        struct __Visitor<'state, __State: ?Sized> {
            state: &'state __State,
        }
    };

    let visit_body = quote! {
        fn visit_seq<__A>(
            self,
            mut __seq: __A,
        ) -> ::core::result::Result<Self::Value, __A::Error>
        where
            __A: _serde::de::SeqAccess<'de>,
        {
            let state = self.state;
            #(#read_fields)*
            ::core::result::Result::Ok(#construct)
        }
    };

    let visitor_where_clause = quote_where_clause(where_clause);
    let visitor_impl = quote! {
        impl<'de, 'state, __State: ?Sized> _serde::de::Visitor<'de> for __Visitor<'state, __State> #visitor_where_clause {
            type Value = #ident;

            fn expecting(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                formatter.write_str("tuple struct ")?;
                formatter.write_str(stringify!(#ident))
            }

            #visit_body
        }
    };

    quote! {
        #visitor_struct
        #visitor_impl

        _serde::Deserializer::deserialize_tuple_struct(
            __deserializer,
            stringify!(#ident),
            #len,
            __Visitor { state: __state },
        )
    }
}

fn deserialize_unit_struct(ident: &syn::Ident) -> TokenStream {
    quote! {
        struct __Visitor;
        impl<'de> _serde::de::Visitor<'de> for __Visitor {
            type Value = #ident;

            fn expecting(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                formatter.write_str("unit struct ")?;
                formatter.write_str(stringify!(#ident))
            }

            fn visit_unit<E>(self) -> ::core::result::Result<Self::Value, E>
            where
                E: _serde::de::Error,
            {
                ::core::result::Result::Ok(#ident)
            }
        }

        _serde::Deserializer::deserialize_unit_struct(
            __deserializer,
            stringify!(#ident),
            __Visitor,
        )
    }
}

fn deserialize_enum_body(
    ident: &syn::Ident,
    data: &DataEnum,
    state_tokens: &TokenStream,
    where_clause: &Option<syn::WhereClause>,
) -> TokenStream {
    let variant_names: Vec<_> = data
        .variants
        .iter()
        .map(|variant| variant.ident.to_string())
        .collect();
    let variant_idents: Vec<_> = data.variants.iter().map(|variant| &variant.ident).collect();

    let const_variants = {
        let names = variant_names.iter();
        quote! {
            const __VARIANTS: &'static [&'static str] = &[#(#names),*];
        }
    };

    let variant_enum = {
        let variants = variant_idents.iter();
        quote! {
            #[allow(non_camel_case_types)]
            enum __Variant { #(#variants),* }
        }
    };

    let variant_visitor = {
        let match_arms = variant_names
            .iter()
            .zip(variant_idents.iter())
            .map(|(name, ident)| {
                quote! { #name => ::core::result::Result::Ok(__Variant::#ident) }
            });
        quote! {
            struct __VariantVisitor;
            impl<'de> _serde::de::Visitor<'de> for __VariantVisitor {
                type Value = __Variant;

                fn expecting(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    formatter.write_str("variant identifier")
                }

                fn visit_str<E>(self, value: &str) -> ::core::result::Result<Self::Value, E>
                where
                    E: _serde::de::Error,
                {
                    match value {
                        #(#match_arms,)*
                        _ => ::core::result::Result::Err(_serde::de::Error::unknown_variant(value, __VARIANTS)),
                    }
                }
            }

            impl<'de> _serde::Deserialize<'de> for __Variant {
                fn deserialize<D>(deserializer: D) -> ::core::result::Result<Self, D::Error>
                where
                    D: _serde::Deserializer<'de>,
                {
                    deserializer.deserialize_identifier(__VariantVisitor)
                }
            }
        }
    };

    let mut helper_tokens = Vec::new();
    let variant_match_arms = data.variants.iter().enumerate().map(|(index, variant)| {
        deserialize_enum_variant_arm(
            ident,
            variant,
            state_tokens,
            index,
            &mut helper_tokens,
            where_clause,
        )
    });

    let visitor_struct = quote! {
        struct __Visitor<'state, __State: ?Sized> {
            state: &'state __State,
        }
    };

    let visitor_where_clause = quote_where_clause(where_clause);
    let visitor_impl = quote! {
        impl<'de, 'state, __State: ?Sized> _serde::de::Visitor<'de> for __Visitor<'state, __State> #visitor_where_clause {
            type Value = #ident;

            fn expecting(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                formatter.write_str("enum ")?;
                formatter.write_str(stringify!(#ident))
            }

            fn visit_enum<__E>(
                self,
                __enum: __E,
            ) -> ::core::result::Result<Self::Value, __E::Error>
            where
                __E: _serde::de::EnumAccess<'de>,
            {
                let state = self.state;
                match _serde::de::EnumAccess::variant::<__Variant>(__enum)? {
                    #(#variant_match_arms)*
                }
            }
        }
    };

    quote! {
        #const_variants
        #variant_enum
        #variant_visitor
        #(#helper_tokens)*
        #visitor_struct
        #visitor_impl

        _serde::Deserializer::deserialize_enum(
            __deserializer,
            stringify!(#ident),
            __VARIANTS,
            __Visitor { state: __state },
        )
    }
}

fn deserialize_enum_variant_arm(
    ident: &syn::Ident,
    variant: &syn::Variant,
    state_tokens: &TokenStream,
    index: usize,
    helpers: &mut Vec<TokenStream>,
    where_clause: &Option<syn::WhereClause>,
) -> TokenStream {
    let variant_ident = &variant.ident;
    match &variant.fields {
        Fields::Unit => {
            quote! {
                (__Variant::#variant_ident, __variant) => {
                    _serde::de::VariantAccess::unit_variant(__variant)?;
                    ::core::result::Result::Ok(#ident::#variant_ident)
                }
            }
        }
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
            let ty = &fields.unnamed.first().unwrap().ty;
            quote! {
                (__Variant::#variant_ident, __variant) => {
                    let __seed = _serde_state::__private::wrap_deserialize_seed::<#ty, #state_tokens>(state);
                    let __value = _serde::de::VariantAccess::newtype_variant_seed(__variant, __seed)?;
                    ::core::result::Result::Ok(#ident::#variant_ident(__value))
                }
            }
        }
        Fields::Unnamed(fields) => {
            let visitor_ident = format_ident!("__Variant{}_TupleVisitor", index);
            helpers.push(tuple_variant_visitor(
                ident,
                variant_ident,
                fields,
                state_tokens,
                &visitor_ident,
                where_clause,
            ));
            let len = fields.unnamed.len();
            quote! {
                (__Variant::#variant_ident, __variant) => {
                    _serde::de::VariantAccess::tuple_variant(
                        __variant,
                        #len,
                        #visitor_ident { state },
                    )
                }
            }
        }
        Fields::Named(fields) => {
            let visitor_ident = format_ident!("__Variant{}_StructVisitor", index);
            let field_array_ident = format_ident!("__VARIANT_FIELDS_{}", index);
            helpers.push(struct_variant_helpers(
                ident,
                variant_ident,
                fields,
                state_tokens,
                &visitor_ident,
                &field_array_ident,
                where_clause,
            ));
            quote! {
                (__Variant::#variant_ident, __variant) => {
                    _serde::de::VariantAccess::struct_variant(
                        __variant,
                        #field_array_ident,
                        #visitor_ident { state },
                    )
                }
            }
        }
    }
}

fn tuple_variant_visitor(
    ident: &syn::Ident,
    variant_ident: &syn::Ident,
    fields: &FieldsUnnamed,
    state_tokens: &TokenStream,
    visitor_ident: &syn::Ident,
    where_clause: &Option<syn::WhereClause>,
) -> TokenStream {
    let len = fields.unnamed.len();
    let bindings: Vec<_> = (0..len)
        .map(|i| format_ident!("__variant_field_{}", i))
        .collect();
    let read_fields = fields.unnamed.iter().enumerate().map(|(index, field)| {
        let binding = &bindings[index];
        let ty = &field.ty;
        let idx = index;
        quote! {
            let #binding = match _serde::de::SeqAccess::next_element_seed(
                &mut __seq,
                _serde_state::__private::wrap_deserialize_seed::<#ty, #state_tokens>(state),
            )? {
                ::core::option::Option::Some(value) => value,
                ::core::option::Option::None =>
                    return ::core::result::Result::Err(_serde::de::Error::invalid_length(#idx, &self)),
            };
        }
    });
    let construct = quote!(#ident::#variant_ident(#(#bindings),*));

    let visitor_struct = quote! {
        #[allow(non_camel_case_types)]
        struct #visitor_ident<'state, __State: ?Sized> {
            state: &'state __State,
        }
    };

    let visit_body = quote! {
        fn visit_seq<__A>(
            self,
            mut __seq: __A,
        ) -> ::core::result::Result<Self::Value, __A::Error>
        where
            __A: _serde::de::SeqAccess<'de>,
        {
            let state = self.state;
            #(#read_fields)*
            ::core::result::Result::Ok(#construct)
        }
    };

    let visitor_where_clause = quote_where_clause(where_clause);
    let visitor_impl = quote! {
        impl<'de, 'state, __State: ?Sized> _serde::de::Visitor<'de> for #visitor_ident<'state, __State> #visitor_where_clause {
            type Value = #ident;

            fn expecting(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                formatter.write_str("tuple variant ")?;
                formatter.write_str(stringify!(#ident::#variant_ident))
            }

            #visit_body
        }
    };

    quote! {
        #visitor_struct
        #visitor_impl
    }
}

fn struct_variant_helpers(
    ident: &syn::Ident,
    variant_ident: &syn::Ident,
    fields: &FieldsNamed,
    state_tokens: &TokenStream,
    visitor_ident: &syn::Ident,
    field_array_ident: &syn::Ident,
    where_clause: &Option<syn::WhereClause>,
) -> TokenStream {
    let field_names: Vec<String> = fields
        .named
        .iter()
        .map(|field| field.ident.as_ref().unwrap().to_string())
        .collect();
    let field_idents: Vec<_> = fields
        .named
        .iter()
        .map(|field| field.ident.as_ref().unwrap())
        .collect();
    let field_variants: Vec<_> = field_names
        .iter()
        .map(|name| format_ident!("__variant_field_{}", name))
        .collect();

    let const_fields = {
        let names = field_names.iter();
        quote! {
            const #field_array_ident: &'static [&'static str] = &[#(#names),*];
        }
    };

    let field_enum_ident = format_ident!("__VariantFieldEnum_{}", variant_ident);
    let field_enum = {
        let variants = field_variants.iter();
        quote! {
            #[allow(non_camel_case_types)]
            enum #field_enum_ident { #(#variants,)* __Ignore }
        }
    };

    let field_visitor_ident = format_ident!("__VariantFieldVisitor_{}", variant_ident);
    let field_visitor = {
        let match_arms = field_names
            .iter()
            .zip(field_variants.iter())
            .map(|(name, variant)| {
                quote! { #name => ::core::result::Result::Ok(#field_enum_ident::#variant) }
            });
        quote! {
            #[allow(non_camel_case_types)]
            struct #field_visitor_ident;
            impl<'de> _serde::de::Visitor<'de> for #field_visitor_ident {
                type Value = #field_enum_ident;

                fn expecting(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    formatter.write_str("field name")
                }

                fn visit_str<E>(self, value: &str) -> ::core::result::Result<Self::Value, E>
                where
                    E: _serde::de::Error,
                {
                    match value {
                        #(#match_arms,)*
                        _ => ::core::result::Result::Ok(#field_enum_ident::__Ignore),
                    }
                }
            }

            impl<'de> _serde::Deserialize<'de> for #field_enum_ident {
                fn deserialize<D>(deserializer: D) -> ::core::result::Result<Self, D::Error>
                where
                    D: _serde::Deserializer<'de>,
                {
                    deserializer.deserialize_identifier(#field_visitor_ident)
                }
            }
        }
    };

    let init_locals = field_idents
        .iter()
        .map(|ident| quote!(let mut #ident = ::core::option::Option::None;));

    let match_arms = fields
        .named
        .iter()
        .zip(field_variants.iter())
        .map(|(field, variant)| {
            let ident = field.ident.as_ref().unwrap();
            let ty = &field.ty;
            let name = ident.to_string();
            quote! {
                #field_enum_ident::#variant => {
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

    let build_fields = field_idents.iter().map(|ident| {
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
        let pairs = field_idents.iter().map(|ident| quote!(#ident: #ident));
        quote!(#ident::#variant_ident { #(#pairs),* })
    };

    let visitor_struct = quote! {
        #[allow(non_camel_case_types)]
        struct #visitor_ident<'state, __State: ?Sized> {
            state: &'state __State,
        }
    };

    let visitor_where_clause = quote_where_clause(where_clause);
    let visitor_impl = quote! {
        impl<'de, 'state, __State: ?Sized> _serde::de::Visitor<'de> for #visitor_ident<'state, __State> #visitor_where_clause {
            type Value = #ident;

            fn expecting(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                formatter.write_str("struct variant ")?;
                formatter.write_str(stringify!(#ident::#variant_ident))
            }

            fn visit_map<__M>(
                self,
                mut __map: __M,
            ) -> ::core::result::Result<Self::Value, __M::Error>
            where
                __M: _serde::de::MapAccess<'de>,
            {
                let state = self.state;
                #(#init_locals)*
                while let ::core::option::Option::Some(key) =
                    _serde::de::MapAccess::next_key::<#field_enum_ident>(&mut __map)?
                {
                    match key {
                        #(#match_arms)*
                        #field_enum_ident::__Ignore => {
                            let _ =
                                _serde::de::MapAccess::next_value::<_serde::de::IgnoredAny>(&mut __map)?;
                        }
                    }
                }
                #(#build_fields)*
                ::core::result::Result::Ok(#construct)
            }
        }
    };

    quote! {
        #const_fields
        #field_enum
        #field_visitor
        #visitor_struct
        #visitor_impl
    }
}

fn add_state_param(generics: &Generics) -> Generics {
    let mut generics = generics.clone();
    let lifetime: syn::LifetimeParam = parse_quote!('de);
    generics.params.insert(0, GenericParam::Lifetime(lifetime));
    generics.params.push(parse_quote!(__State: ?Sized));
    generics
}

fn collect_field_types_from_fields<'a>(fields: &'a Fields) -> Vec<&'a syn::Type> {
    match fields {
        Fields::Named(named) => named
            .named
            .iter()
            .filter(|field| !is_recursive_field(field))
            .map(|field| &field.ty)
            .collect(),
        Fields::Unnamed(unnamed) => unnamed
            .unnamed
            .iter()
            .filter(|field| !is_recursive_field(field))
            .map(|field| &field.ty)
            .collect(),
        Fields::Unit => Vec::new(),
    }
}

fn collect_field_types_from_enum<'a>(data: &'a DataEnum) -> Vec<&'a syn::Type> {
    let mut result = Vec::new();
    for variant in &data.variants {
        result.extend(collect_field_types_from_fields(&variant.fields));
    }
    result
}

fn add_deserialize_bounds_from_types(
    where_clause: &mut Option<syn::WhereClause>,
    field_types: &[&syn::Type],
    state_ty: &TokenStream,
) {
    if field_types.is_empty() {
        return;
    }

    let clause = where_clause.get_or_insert_with(|| syn::WhereClause {
        where_token: Default::default(),
        predicates: Default::default(),
    });

    for ty in field_types {
        clause
            .predicates
            .push(parse_quote!(#ty: _serde_state::DeserializeState<'de, #state_ty>));
    }
}

fn quote_where_clause(clause: &Option<syn::WhereClause>) -> TokenStream {
    match clause {
        Some(clause) => quote!(#clause),
        None => TokenStream::new(),
    }
}

struct ContainerAttributes {
    transparent: bool,
    serde_path: Option<syn::Path>,
}

impl ContainerAttributes {
    fn from_attrs(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut result = ContainerAttributes {
            transparent: false,
            serde_path: None,
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
                    return Err(meta.error(
                        "`serde_state(state = ..)` is no longer supported; the derive now infers the state",
                    ));
                }
                Err(meta.error("unsupported serde attribute"))
            })?;
        }

        Ok(result)
    }
}

fn is_recursive_field(field: &syn::Field) -> bool {
    field.attrs.iter().any(|attr| {
        if attr.path().is_ident("serde_state") {
            let mut recursive = false;
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("recursive") {
                    recursive = true;
                }
                Ok(())
            });
            return recursive;
        }
        false
    })
}

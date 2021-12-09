use proc_macro::TokenStream;
use quote::quote;
use syn::{
    FnArg, GenericArgument, GenericParam, ImplItem, ItemImpl, MetaNameValue, PathArguments,
    PathSegment, ReturnType, Type, TypePath,
};

#[proc_macro_attribute]
pub fn xtra_productivity(attribute: TokenStream, item: TokenStream) -> TokenStream {
    let block = syn::parse::<ItemImpl>(item).unwrap();
    let want_message_impl = if attribute.is_empty() {
        true
    } else {
        let attribute = syn::parse::<MetaNameValue>(attribute).unwrap();
        if !attribute.path.is_ident("message_impl") {
            panic!(
                "Unexpected attribute {:?}",
                attribute.path.get_ident().unwrap()
            )
        }

        matches!(
            attribute.lit,
            syn::Lit::Bool(syn::LitBool { value: true, .. })
        )
    };

    let actor = block.self_ty;

    let generic_params = &block.generics.params;

    let generic_types = block
        .generics
        .params
        .iter()
        .filter_map(|param| match param {
            GenericParam::Type(ty) => Some(ty.ident.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();

    let additional_bounds = block
        .generics
        .where_clause
        .map(|bounds| {
            let predicates = bounds.predicates;

            quote! {
                #predicates
            }
        })
        .unwrap_or_default();

    let code = block
        .items
        .into_iter()
        .filter_map(|block_item| match block_item {
            ImplItem::Method(method) => Some(method),
            _ => None,
        })
        .map(|method| {
            let message_arg = method.sig.inputs[1].clone();

            let message_type = match message_arg {
                // receiver represents self
                FnArg::Receiver(_) => unreachable!("cannot have receiver on second position"),
                FnArg::Typed(ref typed) => typed.ty.clone()
            };

            let method_return = method.sig.output;
            let method_block = method.block;

            let context_arg = method.sig.inputs.iter().nth(2).map(|fn_arg| quote! { #fn_arg }).unwrap_or_else(|| quote! {
                _ctx: &mut xtra::Context<Self>
            });

            let result_type = match method_return {
                ReturnType::Default => quote! { () },
                ReturnType::Type(_, ref t) => quote! { #t }
            };

            let (declaration, where_clause) = match message_type.as_ref() {
                Type::Path(TypePath { path: syn::Path { segments, .. }, .. }) => {
                    if let Some(PathSegment { arguments: PathArguments::AngleBracketed(angle), .. }) = segments.last().cloned() {

                        // filter out actual type parameters
                        let type_parameters = angle.args.into_iter().filter_map(|arg| match arg {
                            GenericArgument::Type(Type::Path(TypePath { path, .. })) => {
                                match path.segments.first() {
                                    Some(only) if path.segments.len() == 1 => {
                                        if only.ident.to_string().len() == 1 { // Heuristic: Single letter idents are type parameters
                                            Some(only.ident.clone())
                                        } else {
                                            None
                                        }
                                    }
                                    _ => None
                                }
                            },
                            _ => None
                        }).collect::<Vec<_>>();

                        let declaration = quote! {
                            <#(#type_parameters),*>
                        };
                        let where_clause = {
                            let bounds = type_parameters.iter().map(|ty| quote! {
                                #ty: Send + 'static,
                            }).collect::<Vec<_>>();

                            quote! {
                                where
                                    #(#bounds)*
                            }
                        };

                        (declaration, where_clause)
                    } else {
                        (quote! {}, quote! {})
                    }
                },
                _ => (quote! {}, quote! {})
            };

            let message_impl = if want_message_impl {
                quote! {
                    impl#declaration xtra::Message for #message_type #where_clause {
                        type Result = #result_type;
                    }
                }
            } else {
                quote! {}
            };

            // dbg!(&message_impl.to_string());

            quote! {
                #message_impl

                #[async_trait::async_trait]
                impl<#generic_params> xtra::Handler<#message_type> for #actor
                    where
                        #additional_bounds
                        #(#generic_types: Send + 'static),*
                {
                    async fn handle(&mut self, #message_arg, #context_arg) #method_return #method_block
                }
            }
        }).collect::<Vec<_>>();

    (quote! {
        #(#code)*
    })
    .into()
}

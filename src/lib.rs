use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, GenericParam, ImplItem, ItemImpl, ReturnType};

#[proc_macro_attribute]
pub fn xtra_productivity(_attribute: TokenStream, item: TokenStream) -> TokenStream {
    let block = syn::parse::<ItemImpl>(item).unwrap();

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

            let mut attrs = method.attrs;

            #[cfg(feature = "instrumentation")]
            let instrument = if !attrs.iter().any(|attr| attr.path.segments.last().unwrap().ident == "instrument") {
                let name = format!("Handle {}", quote!(#message_type));

                let err = if matches!(&*message_type, syn::Type::Path(t) if t.path.segments.last().unwrap().ident == "Result") {
                    quote!(err)
                } else {
                    quote!()
                };

                quote! { #[tracing::instrument(name = #name, skip_all, #err)] }
            } else {
                quote! {}
            };

            #[cfg(not(feature = "instrumentation"))]
            let instrument = quote!();

            let method_block = if let Some(idx) = attrs.iter().position(|attr| attr.path.is_ident("blocking_handler")) {
                let _ = attrs.swap_remove(idx);
                quote!({ ::tokio::task::block_in_place(move || #method_block ) })
            } else {
                quote!(#method_block)
            };

            quote! {
                #[async_trait::async_trait]
                impl<#generic_params> xtra::Handler<#message_type> for #actor
                    where
                        #additional_bounds
                        #(#generic_types: Send + 'static),*
                {
                    type Return = #result_type;
                    #(#attrs)*
                    #instrument
                    async fn handle(&mut self, #message_arg, #context_arg) #method_return #method_block
                }
            }
        }).collect::<Vec<_>>();

    (quote! {
        #(#code)*
    })
    .into()
}

// Copyright 2018-2019 Parity Technologies (UK) Ltd.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use derive_more::From;
use proc_macro2::TokenStream as TokenStream2;
use quote::{
    quote,
    quote_spanned,
};

use crate::{
    codegen::GenerateCode,
    ir,
    ir::utils,
};

/// Generates code to generate the metadata of the contract.
#[derive(From)]
pub struct GenerateAbi<'a> {
    /// The contract to generate code for.
    contract: &'a ir::Contract,
}

impl GenerateCode for GenerateAbi<'_> {
    fn generate_code(&self) -> TokenStream2 {
        let storage_ident = &self.contract.storage.ident;

        let contract = self.generate_contract();
        let layout = self.generate_layout();

        quote! {
            #[cfg(feature = "ink-generate-abi")]
            const _: () = {
                impl ink_lang::GenerateAbi for #storage_ident {
                    fn generate_abi() -> ink_abi::InkProject {
                        let contract: ink_abi::ContractSpec = {
                            #contract
                        };
                        let layout: ink_abi::StorageLayout = {
                            #layout
                        };
                        ink_abi::InkProject::new(layout, contract)
                    }
                }
            };
        }
    }
}

impl GenerateAbi<'_> {
    fn generate_constructors<'a>(&'a self) -> impl Iterator<Item = TokenStream2> + 'a {
        self.contract
            .functions
            .iter()
            .filter_map(|function| {
                function.filter_constructor().map(|kind| (function, kind))
            })
            .map(move |(constructor, kind)| {
                let span = constructor.span();
                let ident_lit = constructor.sig.ident.to_string();
                let selector_bytes = kind.selector.as_bytes();

                let docs = utils::filter_map_trimmed_doc_strings(&constructor.attrs);
                let args = constructor
                    .sig
                    .inputs()
                    .map(|fn_arg| self.generate_message_param(fn_arg));

                quote_spanned!(span =>
                    ink_abi::ConstructorSpec::new(#ident_lit)
                        .selector([#(#selector_bytes),*])
                        .args(vec![
                            #(#args ,)*
                        ])
                        .docs(vec![
                            #(#docs ,)*
                        ])
                        .done()
                )
            })
    }

    fn generate_type_spec(&self, ty: &syn::Type) -> TokenStream2 {
        fn without_display_name(ty: &syn::Type) -> TokenStream2 {
            quote! { ink_abi::TypeSpec::new::<#ty>() }
        }
        if let syn::Type::Path(type_path) = ty {
            if type_path.qself.is_some() {
                return without_display_name(ty)
            }
            let path = &type_path.path;
            if path.segments.is_empty() {
                return without_display_name(ty)
            }
            let segs = path
                .segments
                .iter()
                .map(|seg| seg.ident.to_string())
                .collect::<Vec<_>>();
            quote! {
                ink_abi::TypeSpec::with_name_segs::<#ty, _>(
                    vec![#(#segs),*].into_iter().map(AsRef::as_ref)
                )
            }
        } else {
            without_display_name(ty)
        }
    }

    fn generate_return_type(&self, ret_ty: &syn::ReturnType) -> TokenStream2 {
        match ret_ty {
            syn::ReturnType::Default => {
                quote! {
                    ink_abi::ReturnTypeSpec::new(None)
                }
            }
            syn::ReturnType::Type(_, ty) => {
                let type_spec = self.generate_type_spec(ty);
                quote! {
                    ink_abi::ReturnTypeSpec::new(#type_spec)
                }
            }
        }
    }

    fn generate_message_param(&self, fn_arg: &ir::IdentType) -> TokenStream2 {
        let ident_lit = &fn_arg.ident.to_string();
        let type_spec = self.generate_type_spec(&fn_arg.ty);

        quote! {
            ink_abi::MessageParamSpec::new(#ident_lit)
                .of_type(#type_spec)
                .done()
        }
    }

    fn generate_messages<'a>(&'a self) -> impl Iterator<Item = TokenStream2> + 'a {
        self.contract
            .functions
            .iter()
            .filter_map(|function| function.filter_message().map(|kind| (function, kind)))
            .map(move |(message, kind)| {
                let span = message.span();
                let ident_lit = message.sig.ident.to_string();
                let selector_bytes = kind.selector.as_bytes();
                let is_mut = message.sig.is_mut();

                let docs = utils::filter_map_trimmed_doc_strings(&message.attrs);

                let args = message
                    .sig
                    .inputs()
                    .map(|fn_arg| self.generate_message_param(fn_arg));
                let ret_ty = self.generate_return_type(&message.sig.output);

                quote_spanned!(span =>
                    ink_abi::MessageSpec::new(#ident_lit)
                        .selector([#(#selector_bytes),*])
                        .mutates(#is_mut)
                        .args(vec![
                            #(#args ,)*
                        ])
                        .docs(vec![
                            #(#docs ,)*
                        ])
                        .returns(
                            #ret_ty
                        )
                        .done()
                )
            })
    }

    fn generate_event_args<'a>(
        &'a self,
        event: &'a ir::ItemEvent,
    ) -> impl Iterator<Item = TokenStream2> + 'a {
        event.fields.named.iter().map(move |field| {
            use syn::spanned::Spanned as _;
            let span = field.span();
            let ident = &field
                .ident
                .as_ref()
                .expect("we only operate on named fields");
            let ident_lit = ident.to_string();
            // Query attributes for `#[ink(topic)]` marker.
            use core::convert::TryFrom as _;
            let is_topic = field
                .attrs
                .iter()
                .cloned()
                .filter_map(|attr| ir::Marker::try_from(attr).ok())
                .any(|marker| marker.ident() == "topic");
            let docs = utils::filter_map_trimmed_doc_strings(&field.attrs);
            let ty_spec = self.generate_type_spec(&field.ty);

            quote_spanned!(span =>
                ink_abi::EventParamSpec::new(#ident_lit)
                    .of_type(#ty_spec)
                    .indexed(#is_topic)
                    .docs(vec![
                        #( #docs, )*
                    ])
                    .done()
            )
        })
    }

    fn generate_events<'a>(&'a self) -> impl Iterator<Item = TokenStream2> + 'a {
        self.contract.events.iter().map(move |event| {
            let span = event.span();
            let ident = &event.ident;
            let ident_lit = ident.to_string();

            let docs = utils::filter_map_trimmed_doc_strings(&event.attrs);
            let args = self.generate_event_args(event);

            quote_spanned!(span =>
                ink_abi::EventSpec::new(#ident_lit)
                    .args(vec![
                        #( #args, )*
                    ])
                    .docs(vec![
                        #( #docs, )*
                    ])
                    .done()
            )
        })
    }

    fn generate_docs<'a>(&'a self) -> impl Iterator<Item = String> + 'a {
        utils::filter_map_trimmed_doc_strings(&self.contract.attrs)
    }

    fn generate_contract(&self) -> TokenStream2 {
        let contract_ident_lit = self.contract.ident.to_string();

        let constructors = self.generate_constructors();
        let messages = self.generate_messages();
        let events = self.generate_events();
        let docs = self.generate_docs();

        quote! {
            ink_abi::ContractSpec::new(#contract_ident_lit)
                .constructors(vec![
                    #(#constructors ,)*
                ])
                .messages(vec![
                    #(#messages ,)*
                ])
                .events(vec![
                    #(#events ,)*
                ])
                .docs(vec![
                    #(#docs ,)*
                ])
                .done()
        }
    }

    fn generate_layout(&self) -> TokenStream2 {
        let contract_ident = &self.contract.storage.ident;
        quote! {
            unsafe {
                use ink_abi::HasLayout as _;
                use ink_core::storage::alloc::AllocateUsing as _;
                // We can use `ManuallyDrop` here and don't care for
                // unfreed memory since this function will generally be
                // called from within the `.ink` tool `abi-gen` and process
                // will end shortly after generating the ABI, so the
                // operating system will perform the cleanup immediately
                // for us.
                //
                // # Note
                //
                // This is not an optimization but to prevent panicking
                // because of a potential use of a dynamic environment
                // that uses storage data structures internally
                // that are going to panic upon `Drop` if not initialized
                // beforehand which would normally happen for contract
                // execution.
                core::mem::ManuallyDrop::new(
                    #contract_ident::allocate_using(&mut ink_core::storage::alloc::BumpAlloc::from_raw_parts(
                        ink_primitives::Key([0x0; 32]),
                    ))
                )
                .layout()
            }
        }
    }
}

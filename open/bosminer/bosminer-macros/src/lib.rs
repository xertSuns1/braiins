// Copyright (C) 2019  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::DeriveInput;

#[proc_macro_derive(MiningNode, attributes(member_mining_stats))]
pub fn derive_mining_node(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = syn::parse(input).unwrap();
    impl_derive_mining_node(&ast).into()
}

fn impl_derive_mining_node(ast: &DeriveInput) -> proc_macro2::TokenStream {
    let name = &ast.ident;
    let generics = &ast.generics;

    let fields = get_fields(&ast, "MiningNode");
    let mining_stats = find_member(&fields, "member_mining_stats");

    quote! {
        impl#generics node::Info for #name#generics {}

        impl#generics node::Stats for #name#generics {
            #[inline]
            fn mining_stats(&self) -> &stats::Mining {
                &self.#mining_stats
            }
        }
    }
}

#[proc_macro_derive(
    MiningStats,
    attributes(
        member_start_time,
        member_valid_network_diff,
        member_valid_job_diff,
        member_valid_backend_diff,
        member_error_backend_diff
    )
)]
pub fn derive_mining_stats(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = syn::parse(input).unwrap();
    impl_derive_mining_stats(&ast).into()
}

fn impl_derive_mining_stats(ast: &DeriveInput) -> proc_macro2::TokenStream {
    let name = &ast.ident;
    let generics = &ast.generics;

    let fields = get_fields(&ast, "MiningStats");
    let start_time = find_member(&fields, "member_start_time");
    let valid_network_diff = find_member(&fields, "member_valid_network_diff");
    let valid_job_diff = find_member(&fields, "member_valid_job_diff");
    let valid_backend_diff = find_member(&fields, "member_valid_backend_diff");
    let error_backend_diff = find_member(&fields, "member_error_backend_diff");

    quote! {
        impl#generics stats::Mining for #name#generics {
            #[inline]
            fn start_time(&self) -> &std::time::Instant {
                &self.#start_time
            }

            #[inline]
            fn valid_network_diff(&self) -> &stats::Meter {
                &self.#valid_network_diff
            }

            #[inline]
            fn valid_job_diff(&self) -> &stats::Meter {
                &self.#valid_job_diff
            }

            #[inline]
            fn valid_backend_diff(&self) -> &stats::Meter {
                &self.#valid_backend_diff
            }

            #[inline]
            fn error_backend_diff(&self) -> &stats::Meter {
                &self.#error_backend_diff
            }
        }
    }
}

fn get_fields<'a>(ast: &'a DeriveInput, derive_name: &str) -> &'a syn::Fields {
    match ast.data {
        syn::Data::Struct(ref data) => &data.fields,
        _ => panic!(
            "#[derive({})] can only be used with braced structs",
            derive_name
        ),
    }
}

fn find_member<'a>(fields: &'a syn::Fields, member: &str) -> &'a syn::Ident {
    for field in fields {
        for attr in &field.attrs {
            if attr.path.is_ident(member) {
                return field.ident.as_ref().expect("missing member");
            }
        }
    }
    panic!("missing `{}` attribute", member);
}

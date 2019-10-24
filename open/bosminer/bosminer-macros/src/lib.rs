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

#[proc_macro_derive(MiningStats, attributes(member_mining_stats))]
pub fn derive_stats(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = syn::parse(input).unwrap();
    impl_derive_stats(&ast).into()
}

fn impl_derive_stats(ast: &DeriveInput) -> proc_macro2::TokenStream {
    let name = &ast.ident;
    let generics = &ast.generics;

    let fields: Vec<_> = match ast.data {
        syn::Data::Struct(ref data) => data.fields.iter().collect(),
        _ => panic!("#[derive(Stats)] can only be used with braced structs"),
    };

    let member = find_member(&fields, "member_mining_stats")
        .expect("missing `member_mining_stats` attribute");

    quote! {
        impl#generics node::Stats for #name#generics {
            fn mining_stats(&self) -> &stats::Mining {
                &self.#member
            }
        }
    }
}

fn find_member<'a>(fields: &Vec<&'a syn::Field>, member: &str) -> Option<&'a syn::Ident> {
    for field in fields {
        for attr in &field.attrs {
            if attr.path.is_ident(member) {
                return Some(field.ident.as_ref().expect("missing member"));
            }
        }
    }
    None
}

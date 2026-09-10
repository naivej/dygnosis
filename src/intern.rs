//! Interned identifiers. Diagnostics and lookups share `Name` handles.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Name(u32);

#[derive(Clone, Debug, Default)]
pub struct Interner {
    map: HashMap<String, u32>,
    names: Vec<String>,
}

impl Interner {
    pub fn intern(&mut self, name: &str) -> Name {
        if let Some(&id) = self.map.get(name) {
            return Name(id);
        }
        let id = self.names.len() as u32;
        self.map.insert(name.to_string(), id);
        self.names.push(name.to_string());
        Name(id)
    }

    pub fn get(&self, name: Name) -> &str {
        &self.names[name.0 as usize]
    }
}

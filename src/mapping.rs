//! The mapping store: for every button there is a normal mapping and a second
//! one that applies while the keyboard's SHIFT is held. Mirrors `mapping.c`.

use crate::buttons::REAL_BUTTON_TOTAL;

/// The maximum keys one button can trigger (the C tool has the same limit; a
/// longer list is truncated with a warning instead of overflowing).
pub const MAX_KEYS: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyKind {
    /// A keystroke, sent through uinput.
    Key,
    /// A MIDI Machine Control command, sent through ALSA.
    Mmc,
    /// An MCU button, sent to the DAW on the MCU port. Needs that port: the
    /// plain keystroke mode does not open one, and there is nothing else a
    /// button like this could be sent through.
    Mcu,
}

#[derive(Clone, Copy, Debug)]
pub struct MappedKey {
    pub kind: KeyKind,
    pub key: i32,
}

impl MappedKey {
    pub fn key(code: u16) -> Self {
        MappedKey {
            kind: KeyKind::Key,
            key: i32::from(code),
        }
    }

    pub fn mmc(command: u8) -> Self {
        MappedKey {
            kind: KeyKind::Mmc,
            key: i32::from(command),
        }
    }

    /// An MCU button, by the note the DAW itself uses for it - see
    /// [`crate::mcu_buttons`], where the names and the numbers live.
    pub fn mcu(note: u8) -> Self {
        MappedKey {
            kind: KeyKind::Mcu,
            key: i32::from(note),
        }
    }
}

/// What one button sends: up to `MAX_KEYS` keys, pressed together.
#[derive(Clone, Copy, Debug)]
pub struct MappingKey {
    keys: [Option<MappedKey>; MAX_KEYS],
}

impl Default for MappingKey {
    fn default() -> Self {
        MappingKey {
            keys: [None; MAX_KEYS],
        }
    }
}

impl MappingKey {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a key; returns false when the mapping is full.
    pub fn push(&mut self, key: MappedKey) -> bool {
        for slot in self.keys.iter_mut() {
            if slot.is_none() {
                *slot = Some(key);
                return true;
            }
        }
        false
    }

    pub fn len(&self) -> usize {
        self.keys.iter().filter(|slot| slot.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.iter().all(|slot| slot.is_none())
    }

    pub fn iter(&self) -> impl Iterator<Item = &MappedKey> {
        self.keys.iter().filter_map(|slot| slot.as_ref())
    }
}

/// Which of the two layers is meant.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// Without SHIFT.
    Normal,
    /// With the keyboard's SHIFT held.
    Shifted,
    /// Either of the two (only used to decide whether an LED lights up).
    Any,
}

pub struct MappingTable {
    normal: [MappingKey; REAL_BUTTON_TOTAL],
    shifted: [MappingKey; REAL_BUTTON_TOTAL],
}

impl Default for MappingTable {
    fn default() -> Self {
        MappingTable {
            normal: [MappingKey::new(); REAL_BUTTON_TOTAL],
            shifted: [MappingKey::new(); REAL_BUTTON_TOTAL],
        }
    }
}

impl MappingTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores a mapping; `Layer::Any` is not accepted (nothing would be stored).
    pub fn set(&mut self, index: i32, layer: Layer, key: MappingKey) {
        if index < 0 || index as usize >= REAL_BUTTON_TOTAL {
            return;
        }
        let index = index as usize;
        match layer {
            Layer::Shifted => self.shifted[index] = key,
            _ => self.normal[index] = key,
        }
    }

    /// The mapping of the given layer; an empty mapping for unknown indexes.
    pub fn get(&self, index: i32, layer: Layer) -> MappingKey {
        if index < 0 || index as usize >= REAL_BUTTON_TOTAL {
            return MappingKey::new();
        }
        let index = index as usize;
        match layer {
            Layer::Shifted => self.shifted[index],
            _ => self.normal[index],
        }
    }

    /// True when the button has a mapping in the given layer (`Layer::Any`:
    /// in at least one of them).
    pub fn is_mapped(&self, index: i32, layer: Layer) -> bool {
        if index < 0 || index as usize >= REAL_BUTTON_TOTAL {
            return false;
        }
        let index = index as usize;
        match layer {
            Layer::Normal => !self.normal[index].is_empty(),
            Layer::Shifted => !self.shifted[index].is_empty(),
            Layer::Any => !self.normal[index].is_empty() || !self.shifted[index].is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_reads_layers() {
        let mut table = MappingTable::new();
        let mut key = MappingKey::new();
        assert!(key.push(MappedKey::key(57)));
        assert!(key.push(MappedKey::mmc(2)));
        table.set(0, Layer::Normal, key);
        table.set(3, Layer::Shifted, key);

        assert_eq!(table.get(0, Layer::Normal).len(), 2);
        assert!(table.get(0, Layer::Normal).iter().any(|k| k.kind == KeyKind::Mmc));
        assert!(table.get(0, Layer::Shifted).is_empty());
        assert!(table.is_mapped(0, Layer::Normal));
        assert!(table.is_mapped(0, Layer::Any));
        assert!(!table.is_mapped(0, Layer::Shifted));
        assert!(table.is_mapped(3, Layer::Shifted));
        // Out of range is not a panic.
        assert!(table.get(-1, Layer::Normal).is_empty());
        assert!(!table.is_mapped(99, Layer::Any));
    }

    #[test]
    fn mapping_is_capped_at_four_keys() {
        let mut key = MappingKey::new();
        for code in 0..4 {
            assert!(key.push(MappedKey::key(code)));
        }
        assert!(!key.push(MappedKey::key(4)));
        assert_eq!(key.len(), 4);
    }
}

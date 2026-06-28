//! Persona auto-naming convention.
//!
//! A collaboration's agents share a FIRST LETTER, and are named alphabetically by
//! role: the **executor** (the agent writing code) gets the lowest name, and each
//! reviewer goes up from there. So at a glance you can read the roster:
//!
//!   triad (letter A):  Abbey (executor) · Alice (reviewer) · Annie (reviewer)
//!
//! Different collaborations get different letters, so who's-who stays legible even
//! when several collaborations run at once. The operator can always override with
//! explicit `--persona` names; this just makes the zero-config path pleasant.

/// Six alphabetically-sorted names per letter — enough for an executor plus up to
/// five reviewers. Each list MUST stay sorted: position encodes role seniority.
const TABLE: [(char, [&str; 6]); 26] = [
    ('a', ["Abbey", "Alice", "Annie", "Arlo", "Aspen", "Atlas"]),
    ('b', ["Bailey", "Beck", "Bex", "Birdie", "Bram", "Bruno"]),
    ('c', ["Cady", "Cass", "Cleo", "Cody", "Cora", "Cyrus"]),
    ('d', ["Dahlia", "Dale", "Dax", "Dell", "Dot", "Drew"]),
    ('e', ["Echo", "Eden", "Elia", "Ellis", "Emery", "Ezra"]),
    ('f', ["Fable", "Fern", "Finn", "Flora", "Ford", "Fox"]),
    ('g', ["Gable", "Gale", "Gem", "Gigi", "Grey", "Gus"]),
    ('h', ["Hale", "Harlow", "Haven", "Hazel", "Hollis", "Hugo"]),
    ('i', ["Ibis", "Idris", "Iggy", "Ines", "Iris", "Ivy"]),
    ('j', ["Jade", "Jas", "Jet", "Jonah", "Jules", "Juno"]),
    ('k', ["Kai", "Keaton", "Kira", "Kit", "Knox", "Kona"]),
    ('l', ["Lane", "Lark", "Leo", "Lila", "Lou", "Lux"]),
    ('m', ["Mabel", "Marlo", "Max", "Mira", "Moss", "Murphy"]),
    ('n', ["Nadia", "Nash", "Neo", "Nia", "Noor", "Nova"]),
    ('o', ["Oak", "Odin", "Ola", "Olive", "Orla", "Otis"]),
    ('p', ["Pax", "Penny", "Pip", "Posy", "Pria", "Prim"]),
    ('q', ["Quade", "Quill", "Quin", "Quincy", "Quinn", "Quinta"]),
    ('r', ["Rae", "Reed", "Remy", "Rex", "Rio", "Rowan"]),
    ('s', ["Sage", "Sam", "Shay", "Sol", "Sora", "Sy"]),
    ('t', ["Tao", "Tate", "Teo", "Thea", "Tov", "Tully"]),
    ('u', ["Udo", "Ula", "Ulric", "Uma", "Umi", "Uri"]),
    ('v', ["Vale", "Vera", "Vex", "Vida", "Vin", "Vox"]),
    ('w', ["Wade", "Wells", "Wes", "Wilder", "Wren", "Wyatt"]),
    ('x', ["Xan", "Xander", "Xavi", "Xena", "Ximena", "Xiu"]),
    ('y', ["Yael", "Yana", "Yara", "Yas", "York", "Yuki"]),
    ('z', ["Zara", "Zed", "Zen", "Zia", "Ziggy", "Zoe"]),
];

/// The names for one letter, sorted (executor first).
fn names_for(letter: char) -> Option<&'static [&'static str; 6]> {
    let l = letter.to_ascii_lowercase();
    TABLE.iter().find(|(c, _)| *c == l).map(|(_, names)| names)
}

/// Build a roster of `count` personas sharing `letter`, executor first.
/// For `count > 6`, extra agents get numbered suffixes (e.g. `Atlas2`).
pub fn roster(letter: char, count: usize) -> Vec<String> {
    let base = names_for(letter)
        .copied()
        .unwrap_or(["Ada", "Ben", "Cam", "Dot", "Eli", "Fay"]);
    (0..count)
        .map(|i| {
            if i < base.len() {
                base[i].to_string()
            } else {
                format!("{}{}", base[base.len() - 1], i - base.len() + 2)
            }
        })
        .collect()
}

/// Pick the first letter NOT already used by an existing collaboration, so each
/// collaboration is visually distinct. Falls back to 'a' if all are taken.
pub fn pick_letter(used: &[char]) -> char {
    for (c, _) in TABLE.iter() {
        if !used.contains(c) {
            return *c;
        }
    }
    'a'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_is_alphabetical_and_executor_first() {
        let r = roster('a', 3);
        assert_eq!(r, vec!["Abbey", "Alice", "Annie"]);
        // executor (index 0) sorts before every reviewer.
        let mut sorted = r.clone();
        sorted.sort();
        assert_eq!(r, sorted);
    }

    #[test]
    fn roster_overflows_with_suffixes() {
        let r = roster('a', 8);
        assert_eq!(r.len(), 8);
        assert_eq!(r[6], "Atlas2");
        assert_eq!(r[7], "Atlas3");
    }

    #[test]
    fn pick_letter_avoids_used() {
        assert_eq!(pick_letter(&['a', 'b']), 'c');
        assert_eq!(pick_letter(&[]), 'a');
    }
}

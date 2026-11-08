use rand::prelude::*;

pub const ADJECTIVES: &[&str] = &[
    "Dark", "Light", "Pro", "Noob", "Mega", "Super", "Ghost", "Shadow", "Fire", "Ice", 
    "Toxic", "Iron", "Gold", "Diamond", "Bloody", "Crazy", "Deadly", "Epic", "Fly", 
    "Mad", "Cyber", "Neon", "Void", "Magic", "Secret", "Lost", "Fast", "Slow", "Cool"
];

pub const NOUNS: &[&str] = &[
    "Killer", "Gamer", "Boy", "Girl", "Man", "God", "King", "Lord", "Master", "Ninja", 
    "Sniper", "Warrior", "Wolf", "Bear", "Dragon", "Hunter", "Player", "Star", "Hero", 
    "Beast", "Titan", "Demon", "Angel", "Pirate", "Hacker", "Cat", "Dog", "Fox", "Base"
];

pub fn generate_nick() -> String {
    let mut rng = thread_rng();
    let adj = ADJECTIVES.choose(&mut rng).unwrap();
    let noun = NOUNS.choose(&mut rng).unwrap();
    let num = rng.gen_range(1..=9999);
    
    let format_type = rng.gen_range(0..5);
    match format_type {
        0 => format!("{}{}", adj, noun),
        1 => format!("{}{}{}", adj, noun, num),
        2 => format!("{}_{}", adj, noun),
        3 => format!("{}_{}{}", adj, noun, num),
        _ => format!("{}{}", noun, num),
    }
}
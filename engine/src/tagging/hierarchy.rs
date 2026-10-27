use std::collections::HashMap;
use std::sync::LazyLock;

const PEOPLE_CHILDREN: [&str; 18] = [
    "man",
    "woman",
    "child",
    "baby",
    "boy",
    "girl",
    "teenager",
    "adult",
    "senior",
    "crowd",
    "family",
    "couple",
    "portrait",
    "self-portrait",
    "face",
    "hands",
    "feet",
    "candid",
];

const ANIMAL_CHILDREN: [&str; 30] = [
    "dog", "cat", "bird", "horse", "cow", "sheep", "pig", "goat", "chicken", "duck", "lion",
    "tiger", "bear", "wolf", "fox", "deer", "elephant", "giraffe", "zebra", "monkey", "panda",
    "snake", "lizard", "turtle", "frog", "fish", "shark", "whale", "dolphin", "insect",
];

// Nature & Landscape
const NATURE_CHILDREN: [&str; 28] = [
    "mountain",
    "hill",
    "valley",
    "canyon",
    "desert",
    "forest",
    "jungle",
    "tree",
    "flower",
    "field",
    "meadow",
    "grass",
    "farm",
    "garden",
    "park",
    "beach",
    "coast",
    "ocean",
    "sea",
    "river",
    "lake",
    "waterfall",
    "island",
    "cave",
    "rock",
    "volcano",
    "glacier",
    "snow",
];

// Sky & Weather is covered by the single-entry overrides below.
const ARCHITECTURE_CHILDREN: [&str; 20] = [
    "skyscraper",
    "bridge",
    "tunnel",
    "house",
    "home",
    "apartment",
    "cabin",
    "castle",
    "church",
    "cathedral",
    "tower",
    "lighthouse",
    "ruins",
    "monument",
    "statue",
    "fountain",
    "door",
    "window",
    "interior",
    "room",
];

const VEHICLE_CHILDREN: [&str; 11] = [
    "car",
    "bicycle",
    "motorcycle",
    "bus",
    "train",
    "airplane",
    "boat",
    "ship",
    "truck",
    "van",
    "scooter",
];

const FOOD_CHILDREN: [&str; 17] = [
    "fruit",
    "apple",
    "banana",
    "orange",
    "vegetable",
    "carrot",
    "broccoli",
    "tomato",
    "bread",
    "cake",
    "pizza",
    "pasta",
    "sushi",
    "burger",
    "sandwich",
    "salad",
    "soup",
];

const SINGLES: &[(&str, &[&str])] = &[
    ("boy", &["person", "people", "child"]),
    ("girl", &["person", "people", "child"]),
    ("teenager", &["person", "people", "child"]),
    ("dog", &["animal", "pet"]),
    ("cat", &["animal", "pet"]),
    ("puppy", &["animal", "pet", "dog"]),
    ("kitten", &["animal", "pet", "cat"]),
    ("lion", &["animal", "wildlife", "cat"]),
    ("tiger", &["animal", "wildlife", "cat"]),
    ("butterfly", &["animal", "insect"]),
    ("bee", &["animal", "insect"]),
    ("spider", &["animal", "insect"]),
    ("rose", &["nature", "landscape", "flower"]),
    ("tulip", &["nature", "landscape", "flower"]),
    ("sunflower", &["nature", "landscape", "flower"]),
    ("pine tree", &["nature", "landscape", "tree"]),
    ("palm tree", &["nature", "landscape", "tree"]),
    ("sunrise", &["sky", "sun"]),
    ("sunset", &["sky", "sun"]),
    ("aurora", &["sky", "night sky"]),
    ("milky way", &["sky", "night sky", "galaxy"]),
    ("cityscape", &["city", "urban", "architecture"]),
    ("skyline", &["city", "urban", "architecture"]),
    ("street", &["city", "urban"]),
    ("apple", &["food", "fruit"]),
    ("banana", &["food", "fruit"]),
    ("orange", &["food", "fruit"]),
    ("carrot", &["food", "vegetable"]),
    ("broccoli", &["food", "vegetable"]),
    ("tomato", &["food", "vegetable", "fruit"]),
    ("coffee", &["drink"]),
    ("tea", &["drink"]),
    ("juice", &["drink"]),
    ("wine", &["drink"]),
    ("beer", &["drink"]),
    ("macro", &["close-up"]),
    ("sepia", &["monochrome"]),
    ("black and white", &["monochrome"]),
    ("golden hour", &["lighting", "sunrise", "sunset"]),
    ("blue hour", &["lighting", "sunrise", "sunset"]),
    ("backlighting", &["lighting", "silhouette"]),
    ("drone shot", &["aerial view"]),
];

pub static TAG_HIERARCHY: LazyLock<HashMap<&'static str, Vec<&'static str>>> =
    LazyLock::new(|| {
        let groups: [(&[&str], &[&str]); 6] = [
            (&PEOPLE_CHILDREN, &["person", "people"]),
            (&ANIMAL_CHILDREN, &["animal"]),
            (&NATURE_CHILDREN, &["nature", "landscape"]),
            (&ARCHITECTURE_CHILDREN, &["architecture", "building"]),
            (&VEHICLE_CHILDREN, &["vehicle"]),
            (&FOOD_CHILDREN, &["food"]),
        ];

        groups
            .into_iter()
            .flat_map(|(children, parents)| {
                children
                    .iter()
                    .copied()
                    .map(move |child| (child, parents.to_vec()))
            })
            .chain(
                SINGLES
                    .iter()
                    .copied()
                    .map(|(child, parents)| (child, parents.to_vec())),
            )
            .collect::<HashMap<&str, Vec<&str>>>()
    });

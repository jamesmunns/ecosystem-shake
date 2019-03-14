use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use mvdb::Mvdb;
use serde::{Deserialize, Serialize};
use serde_json;
use walkdir::{Error as WdError, WalkDir};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Normal,
    Build,
    Dev,
}

#[derive(Serialize, Deserialize, Debug)]
struct Dependency {
    name: String,
    req: String,
    features: Vec<String>,
    optional: bool,
    default_features: bool,
    package: Option<String>,
    // target: ?
    kind: Option<Kind>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Krate {
    name: String,
    vers: String,
    deps: Vec<Dependency>,
    cksum: String,
    features: HashMap<String, Vec<String>>,
    yanked: bool,
    // links: Option<String>
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct Database {
    crates: HashMap<String, HashMap<String, Krate>>,
    reverse_deps: HashMap<String, HashSet<String>>,
}

fn main() -> Result<(), WdError> {
    let mut index = Database::default();

    println!("Building forward deps tree...");

    let walker = WalkDir::new("crates.io-index")
        .into_iter()
        .filter_entry(|e| e.file_name().to_str().map(|s| s != ".git").unwrap_or(false));
    for entry in walker.filter(|w| w.as_ref().unwrap().path().is_file()) {
        // println!("{}", entry.as_ref().unwrap().path().display());
        let path = entry.as_ref().unwrap().path();

        if (&format!("{}", path.display())).ends_with(".json") {
            continue;
        }
        let file = File::open(path).unwrap();
        for line in BufReader::new(file).lines() {
            let x: Krate = serde_json::from_str(&line.unwrap()).unwrap();

            if !index.crates.contains_key(&x.name) {
                index.crates.insert(x.name.clone(), HashMap::new());
            }

            index
                .crates
                .get_mut(&x.name)
                .unwrap()
                .insert(x.vers.clone(), x);
        }
    }

    println!("Building reverse deps tree...");

    // initialize with all empty sets
    let mut rindex: HashMap<String, HashSet<String>> = index
        .crates
        .keys()
        .map(|k| (k.to_string(), HashSet::new()))
        .collect();

    for (cr_ident, cr_vers) in index.crates.iter() {
        // TODO: store each version independently?
        for (_vers, cr_data) in cr_vers {
            for dep in cr_data.deps.iter() {
                if let Some(ref kind) = dep.kind {
                    if kind != &Kind::Normal {
                        continue;
                    }
                }

                let name = if let Some(pkg) = &dep.package {
                    pkg.clone()
                } else {
                    dep.name.clone()
                };

                if !rindex.contains_key(&name) {
                    println!("XXX {}", name);
                    rindex.insert(name.clone(), HashSet::new());
                }

                rindex.get_mut(&name).unwrap().insert(cr_ident.clone());
            }
        }
    }

    index.reverse_deps = rindex;

    // let file = Path::new("index.json");
    // let my_data: Mvdb<Database> = Mvdb::from_file_or_default_pretty(&file).unwrap();
    // my_data.access_mut(|db| *db = index).unwrap();

    println!("Building embedded tree...");

    let seed_crates = ["cortex-m"];

    let mut todo_crates = seed_crates
        .iter()
        .map(|sr| sr.to_string())
        .collect::<Vec<String>>();

    let mut emb_index: HashSet<String> = HashSet::new();
    let mut maybe_respider: HashSet<String> = HashSet::new();

    // First pass - spider upwards
    while let Some(cr) = todo_crates.pop() {
        emb_index.insert(cr.to_string());
        println!("{}", cr);
        // let y: () = index.reverse_deps.get(cr)
        if let Some(rdeps) = index.reverse_deps.get(&cr) {
            for rdep in rdeps.iter() {
                if !emb_index.contains(rdep) {
                    println!("  -> {}", rdep);
                    todo_crates.push(rdep.to_string());
                } else {
                    println!("  -x {}", rdep);
                }
            }
        } else {
            println!("{} has no rdeps?!?!", cr);
        }
    }

    let mut idx_sz = 0;

    while idx_sz != emb_index.len() {
        idx_sz = emb_index.len();

        // Second pass - spider downwards
        let mut todo_crates = emb_index
            .iter()
            .map(|sr| sr.to_string())
            .collect::<Vec<String>>();

        while let Some(cr) = todo_crates.pop() {
            for (_ver, cr_info) in index.crates.get(&cr).unwrap().iter() {
                for dep in cr_info.deps.iter() {
                    if let Some(ref kind) = dep.kind {
                        if kind != &Kind::Normal {
                            continue;
                        }
                    }

                    let name = if let Some(pkg) = &dep.package {
                        pkg.clone()
                    } else {
                        dep.name.clone()
                    };

                    if !emb_index.contains(&name) {
                        maybe_respider.insert(name.clone());
                    }

                    emb_index.insert(name);
                }
            }
        }

        for cr in emb_index.iter() {
            let _ = maybe_respider.remove(cr);
        }
    }

    println!("emb_index = {:#?}", emb_index);
    println!("maybe_respider = {:#?}", maybe_respider);
    println!("emb_index.len() = {}", emb_index.len());

    book(&emb_index, &index);

    Ok(())
}

// Top index
// Index by reverse deps
// Index by downloads
// Index alphabetically
// All crates
// ...

fn book(crates: &HashSet<String>, idx: &Database) {
    // sort by most reverse_deps
    let mut by_rdeps: Vec<(usize, String)> = crates
        .iter()
        .map(|cr| {
            let ct = idx
                .reverse_deps
                .get(cr)
                .unwrap()
                .iter()
                .fold(0, |acc, dep| {
                    // Only count reverse deps if they are in the ecosystem
                    if crates.contains(dep) {
                        acc + 1
                    } else {
                        acc
                    }
                });
            (ct, cr.to_string())
        })
        .collect();

    // Sort largest first
    by_rdeps.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

    println!("{:?}", by_rdeps);

    // create a file for each crate
    std::fs::create_dir_all("book/src/crates").unwrap();

    use std::io::prelude::*;

    let fprint = |f: &mut File, strng: String| {
        f.write_all(strng.as_bytes()).unwrap();
    };

    for (_ct, cr) in by_rdeps.iter() {
        let mut file = File::create(&format!("book/src/crates/{}.md", cr)).unwrap();
        fprint(&mut file, format!("# `{}`\n", cr));

        fprint(&mut file, format!("\nDescription!\n"));

        fprint(&mut file, format!("\n**See more info on [crates.io](https://crates.io/crates/{})**\n", cr));

        fprint(&mut file, format!("\n## Dependencies\n\n"));
        let mut all_deps = HashSet::new();
        for (_ver, cr) in idx.crates.get(cr).unwrap().iter() {
            for dep in cr.deps.iter() {
                if let Some(ref kind) = dep.kind {
                    if kind != &Kind::Normal {
                        continue;
                    }
                }

                let name = if let Some(pkg) = &dep.package {
                    pkg.clone()
                } else {
                    dep.name.clone()
                };

                all_deps.insert(name);
            }
        }

        let mut all_deps = all_deps.iter().collect::<Vec<_>>();
        all_deps.sort_unstable();

        for dep in all_deps {
            fprint(&mut file, format!("* [`{0}`](./{0}.md)\n", dep));
        }

        fprint(&mut file, format!("\n## Embedded Rust Reverse Dependencies\n\n"));

        let mut all_rdeps = idx.reverse_deps.get(cr).unwrap().iter().collect::<Vec<_>>();
        all_rdeps.sort_unstable();

        for rdep in all_rdeps {
            if !crates.contains(rdep) {
                continue;
            }
            fprint(&mut file, format!("* [`{0}`](./{0}.md)\n", rdep));
        }

        fprint(&mut file, format!("\n## Non Embedded Rust Reverse Dependencies\n\n"));

        let mut all_rdeps = idx.reverse_deps.get(cr).unwrap().iter().collect::<Vec<_>>();
        all_rdeps.sort_unstable();

        for rdep in all_rdeps {
            if crates.contains(rdep) {
                continue;
            }
            fprint(&mut file, format!("* [`{0}`](./{0}.md)\n", rdep));
        }

        // TODO: List all versions
        // TODO:
    }

    {
        let mut file = File::create("book/src/rdep-index.md").unwrap();

        fprint(&mut file, "# The Embedded Rust Ecosystem\n\n".into());
        fprint(&mut file, "Sorted by Embedded Rust reverse dependencies.\n\n".into());

        fprint(&mut file, "| Reverse Dependencies | Name | Description |\n".into());
        fprint(&mut file, "| :--- | :--- | :--- |\n".into());

        for (ct, cr) in by_rdeps.iter() {
            fprint(&mut file, format!(
                "| {0} | [`{1}`](./crates/{1}.md) | {2} |\n",
                ct,
                cr,
                "TODO!",
            ));
        }
    }

    let mut by_alpha = crates.iter().collect::<Vec<_>>();
    by_alpha.sort_unstable();

    {
        let mut file = File::create("book/src/alpha-index.md").unwrap();

        fprint(&mut file, "# The Embedded Rust Ecosystem\n\n".into());
        fprint(&mut file, "Sorted alphabetically.\n\n".into());

        fprint(&mut file, "| Name | Description |\n".into());
        fprint(&mut file, "| :--- | :--- |\n".into());

        for cr in by_alpha.iter() {
            fprint(&mut file, format!(
                "| [`{0}`](./crates/{0}.md) | {1} |\n",
                cr,
                "TODO!",
            ));
        }
    }

    {
        let mut file = File::create("book/src/SUMMARY.md").unwrap();

        fprint(&mut file, "# The Embedded Rust Ecosystem\n\n".into());

        fprint(&mut file, "- [Alphabetically](./alpha-index.md)\n".into());
        fprint(&mut file, "- [By Reverse Dependencies](./rdep-index.md)\n".into());
        fprint(&mut file, "- [All Crates](./crates.md)\n".into());

        for cr in by_alpha.iter() {
            fprint(&mut file, format!(
                "    - [`{0}`](./crates/{0}.md)\n",
                cr,
            ));
        }
    }


    // List versions

    // List deps

    // List reverse deps

    // Readme contents

    // crate index pages

    // Alpha
    // reverse_deps
    // downloads
}

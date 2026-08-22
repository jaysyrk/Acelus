use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Remedy {
    RemoveJvmArgument { containing: String },
    LowerMemory { megabytes: u32 },
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    pub title: String,
    pub detail: String,
    pub remedy: Remedy,
}

pub fn diagnose(lines: &[String]) -> Option<Diagnosis> {
    lines.iter().find_map(|line| rule(line))
}

fn rule(line: &str) -> Option<Diagnosis> {
    if let Some(option) = between(line, "Unrecognized VM option '", "'") {
        return Some(Diagnosis {
            title: format!("Java does not know the option {option}"),
            detail: format!(
                "This instance passes {option} to Java, and the runtime Acelus provisions does \
                 not accept it. That usually means the flag came from another launcher running a \
                 different build of Java, where it did exist. Removing it lets the game start; it \
                 is a tuning option, not something the game needs."
            ),
            remedy: Remedy::RemoveJvmArgument {
                containing: option.to_string(),
            },
        });
    }

    if line.contains("Could not reserve enough space for") && line.contains("heap") {
        return Some(Diagnosis {
            title: "Java could not reserve the memory this instance asks for".into(),
            detail: "The heap size set for this instance is larger than the machine can give it. \
                     Lowering it lets the game start."
                .into(),
            remedy: Remedy::LowerMemory { megabytes: 4096 },
        });
    }

    if line.contains("java.lang.OutOfMemoryError") {
        return Some(Diagnosis {
            title: "The game ran out of memory".into(),
            detail: "The heap filled up. Raising this instance's memory usually fixes it, and \
                     with many mods installed it is often necessary."
                .into(),
            remedy: Remedy::None,
        });
    }

    None
}

fn between<'a>(line: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = line.find(open)? + open.len();
    let rest = &line[start..];
    let end = rest.find(close)?;
    Some(&rest[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flag_java_does_not_know_is_named_along_with_how_to_be_rid_of_it() {
        let lines = vec![
            "Unrecognized VM option 'ZGenerational'".to_string(),
            "Error: Could not create the Java Virtual Machine.".to_string(),
            "Error: A fatal exception has occurred. Program will exit.".to_string(),
        ];

        let found = diagnose(&lines).expect("this is the whole reason the game did not start");

        assert!(
            found.title.contains("ZGenerational"),
            "naming the flag is the entire value; got {}",
            found.title
        );
        assert_eq!(
            found.remedy,
            Remedy::RemoveJvmArgument {
                containing: "ZGenerational".into()
            }
        );
    }

    #[test]
    fn a_heap_too_large_to_reserve_is_told_apart_from_running_out_of_it() {
        let too_big = vec![
            "Error occurred during initialization of VM".to_string(),
            "Could not reserve enough space for 16777216KB object heap".to_string(),
        ];
        let ran_out = vec!["java.lang.OutOfMemoryError: Java heap space".to_string()];

        assert!(matches!(
            diagnose(&too_big).map(|found| found.remedy),
            Some(Remedy::LowerMemory { .. })
        ));
        assert!(matches!(
            diagnose(&ran_out).map(|found| found.remedy),
            Some(Remedy::None)
        ));
    }

    #[test]
    fn ordinary_output_is_not_diagnosed_as_a_problem() {
        let lines = vec![
            "[main/INFO]: Loading Minecraft 1.21.11 with Fabric Loader 0.18.4".to_string(),
            "[Render thread/INFO]: Setting user: jaysyrk".to_string(),
            "[Render thread/INFO]: OpenAL initialized".to_string(),
        ];
        assert!(diagnose(&lines).is_none());
    }
}

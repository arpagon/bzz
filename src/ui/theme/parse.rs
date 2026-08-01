use ratatui::widgets::BorderType;

use super::{BorderSurface, HighlightGroup, ThemeOptions};

pub(super) fn parse(
    content: &str,
) -> std::result::Result<(ThemeOptions, Vec<String>), toml::de::Error> {
    let root: toml::Table = toml::from_str(content)?;
    let mut parser = Parser::default();
    for (section, value) in &root {
        match section.as_str() {
            "highlight" => match value.as_table() {
                Some(table) => parser.highlights(table),
                None => parser
                    .warnings
                    .push("[highlight] must be a table and was ignored".into()),
            },
            "ui" => match value.as_table() {
                Some(table) => parser.ui(table),
                None => parser
                    .warnings
                    .push("[ui] must be a table and was ignored".into()),
            },
            _ => parser
                .warnings
                .push(format!("[{section}] is unknown and was ignored")),
        }
    }
    Ok((parser.options, parser.warnings))
}

#[derive(Default)]
struct Parser {
    options: ThemeOptions,
    warnings: Vec<String>,
}

impl Parser {
    fn highlights(&mut self, table: &toml::Table) {
        for (name, value) in table {
            let Some(group) = HighlightGroup::from_name(name) else {
                self.warnings
                    .push(format!("[highlight] {name} is unknown and was ignored"));
                continue;
            };
            let Some(fields) = value.as_table() else {
                self.warnings.push(format!(
                    "[highlight.{name}] must be a table and was ignored"
                ));
                continue;
            };
            self.highlight_fields(group, fields);
        }
    }

    fn highlight_fields(&mut self, group: HighlightGroup, fields: &toml::Table) {
        for (field, value) in fields {
            let options = self.options.highlights.entry(group).or_default();
            match field.as_str() {
                "link" => match value.as_str() {
                    Some("none") => options.link = Some(None),
                    Some(name) => match HighlightGroup::from_name(name) {
                        Some(link) => options.link = Some(Some(link)),
                        None => self.warnings.push(format!(
                            "[highlight.{}] link references unknown group {name} and was ignored",
                            group.name()
                        )),
                    },
                    None => self.type_warning(group, field, "a string"),
                },
                "foreground" => match value.as_str() {
                    Some(value) => options.foreground = Some(value.to_owned()),
                    None => self.type_warning(group, field, "a string"),
                },
                "background" => match value.as_str() {
                    Some(value) => options.background = Some(value.to_owned()),
                    None => self.type_warning(group, field, "a string"),
                },
                "bold" => match value.as_bool() {
                    Some(value) => options.bold = Some(value),
                    None => self.type_warning(group, field, "a boolean"),
                },
                "italic" => match value.as_bool() {
                    Some(value) => options.italic = Some(value),
                    None => self.type_warning(group, field, "a boolean"),
                },
                "dim" => match value.as_bool() {
                    Some(value) => options.dim = Some(value),
                    None => self.type_warning(group, field, "a boolean"),
                },
                "underline" => match value.as_bool() {
                    Some(value) => options.underline = Some(value),
                    None => self.type_warning(group, field, "a boolean"),
                },
                "strikethrough" => match value.as_bool() {
                    Some(value) => options.strikethrough = Some(value),
                    None => self.type_warning(group, field, "a boolean"),
                },
                _ => self.warnings.push(format!(
                    "[highlight.{}] {field} is unknown and was ignored",
                    group.name()
                )),
            }
        }
    }

    fn type_warning(&mut self, group: HighlightGroup, field: &str, expected: &str) {
        self.warnings.push(format!(
            "[highlight.{}] {field} must be {expected} and was ignored",
            group.name()
        ));
    }

    fn ui(&mut self, table: &toml::Table) {
        for (field, value) in table {
            if field != "border" {
                self.warnings
                    .push(format!("[ui] {field} is unknown and was ignored"));
                continue;
            }
            match value.as_table() {
                Some(table) => self.borders(table),
                None => self
                    .warnings
                    .push("[ui.border] must be a table and was ignored".into()),
            }
        }
    }

    fn borders(&mut self, table: &toml::Table) {
        for (field, value) in table {
            let surface = if field == "default" {
                None
            } else {
                match BorderSurface::from_name(field) {
                    Some(surface) => Some(surface),
                    None => {
                        self.warnings
                            .push(format!("[ui.border] {field} is unknown and was ignored"));
                        continue;
                    }
                }
            };
            let Some(value) = value.as_str() else {
                self.warnings.push(format!(
                    "[ui.border] {field} must be a string and was ignored"
                ));
                continue;
            };
            let Some(border) = border_type(value) else {
                self.warnings.push(format!(
                    "[ui.border] {field} = {value:?} is not supported and was ignored"
                ));
                continue;
            };
            match surface {
                Some(surface) => self.options.borders.surfaces[surface as usize] = Some(border),
                None => self.options.borders.default = Some(border),
            }
        }
    }
}

fn border_type(name: &str) -> Option<BorderType> {
    match name {
        "plain" => Some(BorderType::Plain),
        "rounded" => Some(BorderType::Rounded),
        "double" => Some(BorderType::Double),
        "thick" => Some(BorderType::Thick),
        "light_double_dashed" => Some(BorderType::LightDoubleDashed),
        "heavy_double_dashed" => Some(BorderType::HeavyDoubleDashed),
        "light_triple_dashed" => Some(BorderType::LightTripleDashed),
        "heavy_triple_dashed" => Some(BorderType::HeavyTripleDashed),
        "light_quadruple_dashed" => Some(BorderType::LightQuadrupleDashed),
        "heavy_quadruple_dashed" => Some(BorderType::HeavyQuadrupleDashed),
        "quadrant_inside" => Some(BorderType::QuadrantInside),
        "quadrant_outside" => Some(BorderType::QuadrantOutside),
        _ => None,
    }
}

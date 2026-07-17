use std::{
    ops::Range,
    sync::{Arc, OnceLock},
};

use crate::{FontStyle, theme::Theme};

pub(crate) fn is_ansi(language: &str) -> bool {
    matches!(language, "ansi")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnsiColor {
    Indexed(u8),
    Palette(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AnsiState {
    foreground: Option<AnsiColor>,
    background: Option<AnsiColor>,
    font_style: FontStyle,
    dim: bool,
    reverse: bool,
}

impl AnsiState {
    pub(crate) const fn has_explicit_style(self) -> bool {
        self.foreground.is_some()
            || self.background.is_some()
            || self.font_style.bits() != 0
            || self.dim
            || self.reverse
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AnsiSpan {
    pub range: Range<usize>,
    pub state: AnsiState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedAnsiStyle {
    pub color: Arc<str>,
    pub background: Option<Arc<str>>,
    pub font_style: FontStyle,
    pub explicit: bool,
}

pub(crate) fn parse_line(
    line: &str,
    state: &mut AnsiState,
    output: &mut Vec<AnsiSpan>,
) {
    output.clear();
    let bytes = line.as_bytes();
    let mut plain_start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != 0x1b {
            index += 1;
            continue;
        }
        push_span(output, plain_start..index, *state);
        index += 1;
        if index >= bytes.len() {
            plain_start = bytes.len();
            break;
        }
        match bytes[index] {
            b'[' => {
                index += 1;
                let parameters_start = index;
                while index < bytes.len()
                    && !(0x40..=0x7e).contains(&bytes[index])
                {
                    index += 1;
                }
                if index == bytes.len() {
                    plain_start = bytes.len();
                    break;
                }
                if bytes[index] == b'm' {
                    // CSI parameters are ASCII by definition. Invalid bytes are
                    // ignored without ever indexing the source as UTF-8.
                    if let Ok(parameters) =
                        std::str::from_utf8(&bytes[parameters_start..index])
                    {
                        apply_sgr(parameters, state);
                    }
                }
                index += 1;
                plain_start = index;
            }
            b']' => {
                // OSC: discard through BEL or the ESC \\ string terminator.
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == 0x07 {
                        index += 1;
                        break;
                    }
                    if bytes[index] == 0x1b
                        && bytes.get(index + 1) == Some(&b'\\')
                    {
                        index += 2;
                        break;
                    }
                    index += 1;
                }
                plain_start = index;
            }
            _ => {
                // Other two-byte escape commands do not produce visible text.
                index += 1;
                plain_start = index;
            }
        }
    }
    push_span(output, plain_start..line.len(), *state);
}

fn push_span(
    output: &mut Vec<AnsiSpan>,
    range: Range<usize>,
    state: AnsiState,
) {
    if range.is_empty() {
        return;
    }
    if let Some(previous) = output.last_mut()
        && previous.range.end == range.start
        && previous.state == state
    {
        previous.range.end = range.end;
    } else {
        output.push(AnsiSpan { range, state });
    }
}

fn apply_sgr(parameters: &str, state: &mut AnsiState) {
    const MAX_PARAMETERS: usize = 32;
    let mut values = [None; MAX_PARAMETERS];
    let mut len = 0;
    if parameters.is_empty() {
        values[0] = Some(0);
        len = 1;
    } else {
        for value in parameters.split([';', ':']) {
            if len == MAX_PARAMETERS {
                break;
            }
            values[len] = if value.is_empty() {
                None
            } else {
                value.parse::<u16>().ok()
            };
            len += 1;
        }
    }

    let mut index = 0;
    while index < len {
        let code = values[index].unwrap_or(0);
        index += 1;
        match code {
            0 => *state = AnsiState::default(),
            1 => set_font_style(&mut state.font_style, FontStyle::BOLD),
            2 => state.dim = true,
            3 => set_font_style(&mut state.font_style, FontStyle::ITALIC),
            4 | 21 => {
                set_font_style(&mut state.font_style, FontStyle::UNDERLINE)
            }
            7 => state.reverse = true,
            9 => {
                set_font_style(&mut state.font_style, FontStyle::STRIKETHROUGH)
            }
            22 => {
                clear_font_style(&mut state.font_style, FontStyle::BOLD);
                state.dim = false;
            }
            23 => clear_font_style(&mut state.font_style, FontStyle::ITALIC),
            24 => clear_font_style(&mut state.font_style, FontStyle::UNDERLINE),
            27 => state.reverse = false,
            29 => clear_font_style(
                &mut state.font_style,
                FontStyle::STRIKETHROUGH,
            ),
            30..=37 => {
                state.foreground = Some(AnsiColor::Indexed((code - 30) as u8))
            }
            38 => {
                state.foreground =
                    parse_extended_color(&values, len, &mut index)
                        .or(state.foreground);
            }
            39 => state.foreground = None,
            40..=47 => {
                state.background = Some(AnsiColor::Indexed((code - 40) as u8))
            }
            48 => {
                state.background =
                    parse_extended_color(&values, len, &mut index)
                        .or(state.background);
            }
            49 => state.background = None,
            58 => {
                // Underline color is not represented by Shiki's token style,
                // but its parameters still belong to this SGR command.
                let _ = parse_extended_color(&values, len, &mut index);
            }
            59 => {}
            90..=97 => {
                state.foreground =
                    Some(AnsiColor::Indexed((code - 90 + 8) as u8))
            }
            100..=107 => {
                state.background =
                    Some(AnsiColor::Indexed((code - 100 + 8) as u8))
            }
            _ => {}
        }
    }
}

fn parse_extended_color(
    values: &[Option<u16>],
    len: usize,
    index: &mut usize,
) -> Option<AnsiColor> {
    let mode = values.get(*index).copied().flatten()?;
    *index += 1;
    match mode {
        5 => {
            let value = values.get(*index).copied().flatten();
            *index = index.saturating_add(1).min(len);
            let value = value?;
            Some(AnsiColor::Palette(value.min(255) as u8))
        }
        2 => {
            // ISO-8613-6 colon notation may include an empty color-space slot:
            // 38:2::R:G:B. Semicolon notation starts with R immediately.
            if values.get(*index) == Some(&None)
                && len.saturating_sub(*index) >= 4
            {
                *index += 1;
            }
            let red = values.get(*index).copied().flatten();
            let green = values.get(*index + 1).copied().flatten();
            let blue = values.get(*index + 2).copied().flatten();
            *index = index.saturating_add(3).min(len);
            Some(AnsiColor::Rgb(
                red?.min(255) as u8,
                green?.min(255) as u8,
                blue?.min(255) as u8,
            ))
        }
        _ => None,
    }
}

fn set_font_style(style: &mut FontStyle, value: FontStyle) {
    *style = FontStyle::from_bits(style.bits() | value.bits());
}

fn clear_font_style(style: &mut FontStyle, value: FontStyle) {
    *style = FontStyle::from_bits(style.bits() & !value.bits());
}

pub(crate) fn resolve_style(
    theme: &Theme,
    state: AnsiState,
) -> ResolvedAnsiStyle {
    let (foreground, background) = if state.reverse {
        (
            state
                .background
                .map(|color| resolve_color(theme, color))
                .unwrap_or_else(|| theme.background.clone()),
            Some(
                state
                    .foreground
                    .map(|color| resolve_color(theme, color))
                    .unwrap_or_else(|| theme.foreground.clone()),
            ),
        )
    } else {
        (
            state
                .foreground
                .map(|color| resolve_color(theme, color))
                .unwrap_or_else(|| theme.foreground.clone()),
            state.background.map(|color| resolve_color(theme, color)),
        )
    };
    ResolvedAnsiStyle {
        color: if state.dim {
            dim_color(foreground)
        } else {
            foreground
        },
        background,
        font_style: state.font_style,
        explicit: state.has_explicit_style(),
    }
}

fn resolve_color(theme: &Theme, color: AnsiColor) -> Arc<str> {
    match color {
        AnsiColor::Indexed(index) => {
            theme.color_arc(theme.ansi_colors[usize::from(index.min(15))])
        }
        AnsiColor::Palette(index @ 0..=15) => {
            theme.color_arc(theme.ansi_colors[usize::from(index)])
        }
        AnsiColor::Palette(index @ 16..=231) => {
            xterm_colors()[usize::from(index - 16)].clone()
        }
        AnsiColor::Palette(index) => {
            xterm_colors()[usize::from(index - 16)].clone()
        }
        AnsiColor::Rgb(red, green, blue) => {
            Arc::from(format!("#{red:02x}{green:02x}{blue:02x}"))
        }
    }
}

fn xterm_colors() -> &'static [Arc<str>; 240] {
    static COLORS: OnceLock<[Arc<str>; 240]> = OnceLock::new();
    COLORS.get_or_init(|| {
        std::array::from_fn(|index| {
            let color_index = index + 16;
            if color_index <= 231 {
                let cube = (color_index - 16) as u8;
                let red = color_cube(cube / 36);
                let green = color_cube(cube / 6 % 6);
                let blue = color_cube(cube % 6);
                Arc::from(format!("#{red:02x}{green:02x}{blue:02x}"))
            } else {
                let value = 8 + (color_index - 232) * 10;
                Arc::from(format!("#{value:02x}{value:02x}{value:02x}"))
            }
        })
    })
}

const fn color_cube(value: u8) -> u8 {
    if value == 0 { 0 } else { 55 + value * 40 }
}

fn dim_color(color: Arc<str>) -> Arc<str> {
    let Some(hex) = color.strip_prefix('#') else {
        if let Some(variable) = color
            .strip_prefix("var(")
            .and_then(|value| value.strip_suffix(')'))
            && variable.contains("-ansi-")
        {
            return Arc::from(format!("var({variable}-dim)"));
        }
        return color;
    };
    if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return color;
    }
    let output = match hex.len() {
        3 => format!(
            "#{}{}{}{}{}{}80",
            &hex[0..1],
            &hex[0..1],
            &hex[1..2],
            &hex[1..2],
            &hex[2..3],
            &hex[2..3]
        ),
        4 => {
            let alpha = u8::from_str_radix(&format!("{0}{0}", &hex[3..4]), 16)
                .expect("validated hex")
                .div_ceil(2);
            format!(
                "#{}{}{}{}{}{}{alpha:02x}",
                &hex[0..1],
                &hex[0..1],
                &hex[1..2],
                &hex[1..2],
                &hex[2..3],
                &hex[2..3]
            )
        }
        6 => format!("#{hex}80"),
        8 => {
            let alpha = u8::from_str_radix(&hex[6..8], 16)
                .expect("validated hex")
                .div_ceil(2);
            format!("#{}{alpha:02x}", &hex[..6])
        }
        _ => return color,
    };
    Arc::from(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sgr_and_discards_control_sequences() {
        let mut state = AnsiState::default();
        let mut spans = Vec::new();
        let line = "plain\x1b[1;31mred\x1b[0m\x1b]0;title\x07end";
        parse_line(line, &mut state, &mut spans);
        assert_eq!(spans.len(), 3);
        assert_eq!(&line[spans[0].range.clone()], "plain");
        assert_eq!(&line[spans[1].range.clone()], "red");
        assert!(spans[1].state.font_style.contains(FontStyle::BOLD));
        assert_eq!(&line[spans[2].range.clone()], "end");
        assert_eq!(state, AnsiState::default());
    }
}

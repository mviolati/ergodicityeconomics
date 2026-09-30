//! Everything the page says, as text: headline figures, legend, table, tooltip. The page's
//! JavaScript only puts these strings into elements (as text, never as HTML).

use coin_chart::{legend_ticks, rich_label, Layout};
use coin_core::{
    ensemble::Ensemble,
    fmt::{eur, int, pct},
    sim::Summary,
    stats::{Pick, Role, Stats},
    Game,
};

/// Minimal JSON writer: strings are escaped, numbers are written as given.
pub struct Json(String);

impl Json {
    pub fn new() -> Self {
        Json(String::new())
    }
    pub fn raw(&mut self, s: &str) -> &mut Self {
        self.0.push_str(s);
        self
    }
    pub fn str(&mut self, s: &str) -> &mut Self {
        self.0.push('"');
        for c in s.chars() {
            match c {
                '"' => self.0.push_str("\\\""),
                '\\' => self.0.push_str("\\\\"),
                '\n' => self.0.push_str("\\n"),
                c if (c as u32) < 0x20 => self.0.push_str(&format!("\\u{:04x}", c as u32)),
                c => self.0.push(c),
            }
        }
        self.0.push('"');
        self
    }
    pub fn key(&mut self, k: &str) -> &mut Self {
        self.str(k).raw(":")
    }
    pub fn num(&mut self, v: f64) -> &mut Self {
        self.raw(&if v.is_finite() { format!("{v}") } else { "null".into() })
    }
    pub fn finish(self) -> String {
        self.0
    }
}

impl Default for Json {
    fn default() -> Self {
        Self::new()
    }
}

/// The opening paragraph, from the game parameters.
pub fn lede(game: &Game) -> String {
    let up = ((game.win - 1.0) * 100.0).round();
    let down = ((1.0 - game.lose) * 100.0).round();
    let ev = ((0.5 * (game.win + game.lose) - 1.0) * 100.0 * 10.0).round() / 10.0;
    format!(
        "Ogni round si lancia una moneta per ogni giocatore: testa, la sua ricchezza sale del {up}%; croce, scende del {down}%. \
         Tutti partono da {}. Il valore atteso cresce del {}% a round. Ogni percorso è simulato qui, nel tuo browser.",
        eur(game.start.log10()),
        ev.to_string().replace('.', ",")
    )
}

/// "1 giocatore", "12 giocatori".
fn giocatori(n: u64) -> String {
    if n == 1 {
        "1 giocatore".into()
    } else {
        format!("{} giocatori", int(n))
    }
}

fn thr(v: f64) -> String {
    format!("{} €", rich_label(v))
}

/// Name and one-line detail of a highlighted player.
pub fn player_text(game: &Game, s: &Summary, p: &Pick) -> (String, String) {
    let lat = game.lattice();
    let i = p.id as usize;
    let (rich, broke) = (thr(game.rich), thr(game.broke));
    let name = match p.role {
        Role::RichestAtEnd => "Il più ricco alla fine".to_string(),
        Role::RichThenBroke => format!("Da {rich} a sotto {broke}"),
        Role::RichThenLowest => format!("Tra chi ha toccato {rich}, il più in basso alla fine"),
        Role::BiggestFall => "La caduta più grande dal proprio picco".to_string(),
        Role::FirstBroke => format!("Il primo a scendere sotto {broke}"),
    };
    let tie = match (p.role, p.ties) {
        (_, 0) => String::new(),
        (Role::FirstBroke, 1) => " (nello stesso round di un altro giocatore)".to_string(),
        (Role::FirstBroke, n) => format!(" (nello stesso round di altri {})", int(n)),
        (_, 1) => " (a pari merito con un altro giocatore)".to_string(),
        (_, n) => format!(" (a pari merito con altri {})", int(n)),
    };
    let first_broke = if s.broke_t[i] == 0 {
        format!("mai sotto {broke}")
    } else {
        format!("prima volta sotto {broke} al round {}", int(u64::from(s.broke_t[i])))
    };
    let role_note = match p.role {
        Role::RichThenBroke => format!(" · tra chi ha toccato {rich}, il più in basso alla fine"),
        _ => String::new(),
    };
    let detail = format!(
        "#{} · picco {} al round {} · fine {} · {}{}{}",
        int(p.id),
        eur(lat.at(s.peak_t[i], s.peak_k[i])),
        int(u64::from(s.peak_t[i])),
        eur(lat.at(game.rounds, s.final_k[i])),
        first_broke,
        role_note,
        tie
    );
    (name, detail)
}

/// The whole page text for one finished run.
#[allow(clippy::too_many_arguments)]
pub fn view_json(
    game: &Game,
    s: &Summary,
    e: &Ensemble,
    st: &Stats,
    picks: &[Pick],
    rich_shown: usize,
    sim_ms: f64,
    threads: u32,
) -> String {
    let (rich, broke) = (thr(game.rich), thr(game.broke));
    let p = st.players;
    let mut j = Json::new();
    j.raw("{");
    j.key("lede").str(&lede(game)).raw(",");
    j.key("summary")
        .str(&format!("{} giocatori × {} round · seed {}", int(p), int(u64::from(game.rounds)), game.seed))
        .raw(",");

    // Headline figures.
    let tiles: Vec<(String, String)> = vec![
        (int(st.rich_ever), format!("hanno toccato {rich} almeno una volta ({} dei giocatori)", pct(st.rich_ever, p))),
        if st.rich_ever > 0 {
            (
                format!("{} di {}", int(st.rich_ever_broke_at_end), int(st.rich_ever)),
                format!("di questi, sotto {broke} alla fine"),
            )
        } else {
            ("–".into(), format!("nessuno ha toccato {rich}, quindi nessuno da contare qui"))
        },
        if st.rich_most_at_once > 0 {
            (
                int(st.rich_most_at_once),
                format!(
                    "al massimo sopra {rich} nello stesso round (round {})",
                    int(u64::from(st.rich_most_at_once_round))
                ),
            )
        } else {
            ("0".into(), format!("in nessun round qualcuno è sopra {rich}"))
        },
        (int(st.never_broke), format!("mai scesi sotto {broke} ({})", pct(st.never_broke, p))),
        (
            pct(st.above_start_at_end, p),
            format!("sopra i {} iniziali alla fine ({})", eur(game.start.log10()), giocatori(st.above_start_at_end)),
        ),
    ];
    j.key("tiles").raw("[");
    for (n, (v, l)) in tiles.iter().enumerate() {
        if n > 0 {
            j.raw(",");
        }
        j.raw("{").key("value").str(v).raw(",").key("label").str(l).raw("}");
    }
    j.raw("],");

    // Highlighted players.
    j.key("players").raw("[");
    for (n, pk) in picks.iter().enumerate() {
        let (name, detail) = player_text(game, s, pk);
        if n > 0 {
            j.raw(",");
        }
        j.raw("{")
            .key("slot")
            .num(n as f64)
            .raw(",")
            .key("name")
            .str(&name)
            .raw(",")
            .key("detail")
            .str(&detail)
            .raw("}");
    }
    j.raw("],");

    // Rich players' lines.
    let rich_note = if st.rich_ever == 0 {
        format!("Nessun giocatore ha toccato {rich}: nessuna linea arancione.")
    } else if st.rich_ever == 1 {
        format!("Linea arancione sottile: l'unico giocatore che ha toccato {rich}, nei round in cui è sopra.")
    } else if rich_shown < st.rich_ever as usize {
        format!(
            "Linee arancioni sottili: i giocatori sopra {rich} in quel round. Disegnati {} dei {} che lo hanno toccato (i primi per numero).",
            int(rich_shown as u64),
            int(st.rich_ever)
        )
    } else {
        format!(
            "Linee arancioni sottili: i giocatori sopra {rich} in quel round, al massimo {} insieme; {} diversi in tutto.",
            int(st.rich_most_at_once),
            int(st.rich_ever)
        )
    };
    j.key("rich").raw("{").key("count").num(st.rich_ever as f64).raw(",").key("note").str(&rich_note).raw(",");
    let toggle = if rich_shown == 1 {
        format!("Mostra il percorso completo dell'unico giocatore che ha toccato {rich}")
    } else {
        format!("Mostra i percorsi completi dei {} giocatori che hanno toccato {rich}", int(rich_shown as u64))
    };
    j.key("toggle").str(&toggle).raw("},");

    let step = ((0.5 * (game.win + game.lose) - 1.0) * 100.0 * 10.0).round() / 10.0;
    j.key("lines").raw("[");
    j.raw("{").key("style").str("solid").raw(",").key("label").str("Giocatore mediano").raw("},");
    j.raw("{").key("style").str("dot").raw(",").key("label").str("Media di tutti i giocatori").raw("},");
    j.raw("{")
        .key("style")
        .str("dash")
        .raw(",")
        .key("label")
        .str(&format!("Valore atteso (+{}% a round)", step.to_string().replace('.', ",")))
        .raw("}");
    j.raw("],");

    j.key("scale")
        .raw("{")
        .key("title")
        .str("Giocatori nella stessa cella (round × livello di ricchezza), scala logaritmica")
        .raw(",");
    j.key("ticks").raw("[");
    for (n, (v, pos)) in legend_ticks(e.max_cell).iter().enumerate() {
        if n > 0 {
            j.raw(",");
        }
        j.raw("{").key("label").str(&int(u64::from(*v))).raw(",").key("pos").num(*pos).raw("}");
    }
    j.raw("]},");

    // Table view.
    j.key("table").raw("{").key("head").raw("[");
    for (n, h) in
        ["Round", "Giocatore mediano", "Media", "Valore atteso", &format!("Sopra {rich}"), &format!("Sotto {broke}")]
            .iter()
            .enumerate()
    {
        if n > 0 {
            j.raw(",");
        }
        j.str(h);
    }
    j.raw("],").key("rows").raw("[");
    let step_t = (game.rounds / 10).max(1);
    let mut ts: Vec<u32> = (0..=game.rounds).step_by(step_t as usize).collect();
    if ts.last() != Some(&game.rounds) {
        ts.push(game.rounds);
    }
    for (n, &t) in ts.iter().enumerate() {
        let i = t as usize;
        if n > 0 {
            j.raw(",");
        }
        j.raw("[");
        let cells = [
            int(u64::from(t)),
            eur(e.median[i]),
            eur(e.mean[i]),
            eur(e.expected[i]),
            int(e.rich_now[i]),
            int(e.broke_now[i]),
        ];
        for (m, c) in cells.iter().enumerate() {
            if m > 0 {
                j.raw(",");
            }
            j.str(c);
        }
        j.raw("]");
    }
    j.raw("]},");

    j.key("aria").str(&format!(
        "Ricchezza di {} giocatori in {} round, scala logaritmica. Alla fine il giocatore mediano ha {}, la media è {}, il valore atteso {}.",
        int(p),
        int(u64::from(game.rounds)),
        eur(e.median[game.rounds as usize]),
        eur(e.mean[game.rounds as usize]),
        eur(e.expected[game.rounds as usize])
    ));
    j.raw(",");
    j.key("timing").str(&format!(
        "{} lanci di moneta · simulazione {} ms su {} thread",
        int(p * u64::from(game.rounds)),
        int(sim_ms.round().max(0.0) as u64),
        threads
    ));
    j.raw("}");
    j.finish()
}

/// Tooltip at CSS point (x, y) of the chart. `round` and `cell` come from the same pixel
/// ownership rules as the density.
#[allow(clippy::too_many_arguments)]
pub fn hover_json(
    game: &Game,
    e: &Ensemble,
    lay: &Layout,
    highlighted: &[(u64, Vec<f64>)],
    round: u32,
    shared: Option<(u32, u32)>,
    cell: Option<(f64, u32)>,
) -> String {
    let t = round as usize;
    let mut j = Json::new();
    j.raw("{").key("inside").raw("true,").key("x").num(f64::from(lay.x_of(f64::from(round)))).raw(",");
    j.key("top").num(f64::from(lay.y)).raw(",").key("bottom").num(f64::from(lay.y + lay.h)).raw(",");
    let head = match shared {
        Some((a, b)) if a != b => {
            format!("Round {} (questo pixel copre i round {}–{})", int(t as u64), int(u64::from(a)), int(u64::from(b)))
        }
        _ => format!("Round {}", int(t as u64)),
    };
    j.key("head").str(&head).raw(",");
    j.key("players").raw("[");
    for (n, (id, path)) in highlighted.iter().enumerate() {
        if n > 0 {
            j.raw(",");
        }
        j.raw("{").key("slot").num(n as f64).raw(",").key("label").str(&format!("#{}", int(*id))).raw(",");
        j.key("value").str(&eur(path[t])).raw(",").key("y").num(f64::from(lay.y_of(path[t]))).raw("}");
    }
    j.raw("],").key("rows").raw("[");
    let rich = thr(game.rich);
    let rows = [
        ("Giocatore mediano".to_string(), eur(e.median[t])),
        ("Media".to_string(), eur(e.mean[t])),
        ("Valore atteso".to_string(), eur(e.expected[t])),
        (format!("Sopra {rich} in questo round"), int(e.rich_now[t])),
    ];
    for (n, (l, v)) in rows.iter().enumerate() {
        if n > 0 {
            j.raw(",");
        }
        j.raw("{").key("label").str(l).raw(",").key("value").str(v).raw("}");
    }
    j.raw("],").key("cell");
    match cell {
        Some((l, n)) => {
            j.raw("{")
                .key("label")
                .str(&format!("Giocatori a {} in questo round", eur(l)))
                .raw(",")
                .key("value")
                .str(&int(u64::from(n)))
                .raw("}");
        }
        None => {
            j.raw("null");
        }
    }
    j.raw("}");
    j.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_strings_are_escaped() {
        let mut j = Json::new();
        j.str("a\"b\\c\nd\u{1}");
        assert_eq!(j.finish(), r#""a\"b\\c\nd\u0001""#);
    }

    #[test]
    fn lede_states_the_rules() {
        let l = lede(&Game::peters(1000, 1));
        assert!(l.contains("sale del 50%") && l.contains("scende del 40%") && l.contains("100 €") && l.contains("5%"));
    }
}

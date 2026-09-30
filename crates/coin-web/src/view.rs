//! Everything the page says, as text: headline figures, legend, table, tooltip. The page's
//! JavaScript only puts these strings into elements (as text, never as HTML).

use crate::Probe;
use coin_chart::{legend_ticks, rich_label, Layout};
use coin_core::{
    ensemble::Ensemble,
    fmt::{eur, int, pct},
    sim::{Summary, BROKE_T, FALL_FROM_K, FALL_FROM_T, FALL_TO_K, FALL_TO_T, FINAL_K, PEAK_K, PEAK_T},
    stats::{Pick, Role, Stats},
    Game,
};

/// Minimal JSON writer: strings are escaped, numbers are written as given.
#[derive(Default)]
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
    /// A JSON array of strings.
    pub fn strs<S: AsRef<str>>(&mut self, items: &[S]) -> &mut Self {
        self.raw("[");
        for (n, s) in items.iter().enumerate() {
            if n > 0 {
                self.raw(",");
            }
            self.str(s.as_ref());
        }
        self.raw("]")
    }
    pub fn finish(self) -> String {
        self.0
    }
}

/// "1 giocatore", "12 giocatori".
pub fn giocatori(n: u64) -> String {
    if n == 1 {
        "1 giocatore".into()
    } else {
        format!("{} giocatori", int(n))
    }
}

/// "un altro giocatore", "altri 12 giocatori".
pub fn altri(n: u64) -> String {
    if n == 1 {
        "un altro giocatore".into()
    } else {
        format!("altri {} giocatori", int(n))
    }
}

fn percent_step(game: &Game) -> String {
    let v = ((0.5 * (game.win + game.lose) - 1.0) * 1000.0).round() / 10.0;
    v.to_string().replace('.', ",")
}

/// The opening paragraph, from the game parameters.
pub fn lede(game: &Game) -> String {
    let up = ((game.win - 1.0) * 100.0).round();
    let down = ((1.0 - game.lose) * 100.0).round();
    format!(
        "Ogni round si lancia una moneta per ogni giocatore: testa, la sua ricchezza sale del {up}%; croce, scende del {down}%. \
         Tutti partono da {}. Il valore atteso cresce del {}% a round. Ogni percorso è simulato qui, nel tuo browser.",
        eur(game.start.log10()),
        percent_step(game)
    )
}

fn thr(v: f64) -> String {
    format!("{} €", rich_label(v))
}

/// Name and one-line detail of a highlighted player.
pub fn player_text(game: &Game, s: &Summary, p: &Pick) -> (String, String) {
    let lat = game.lattice();
    let i = p.id as usize;
    let (rich, broke) = (thr(game.rich), thr(game.broke));
    let tie = if p.ties > 0 { format!(" (a pari merito con {})", altri(p.ties)) } else { String::new() };
    let broke_t = s.get(BROKE_T, i);
    let name = match p.role {
        Role::RichestAtEnd => format!("Il più ricco alla fine{tie}"),
        Role::RichThenBroke => format!("Da {rich} a sotto {broke}{tie}"),
        Role::RichThenLowest => format!("Tra chi ha toccato {rich}, il più in basso alla fine{tie}"),
        Role::BiggestFall => format!("La caduta più grande{tie}"),
        Role::FirstBroke if p.ties > 0 => format!(
            "Tra i primi a scendere sotto {broke}: al round {}, insieme ad {}",
            int(u64::from(broke_t)),
            altri(p.ties)
        ),
        Role::FirstBroke => format!("Il primo a scendere sotto {broke}: al round {}", int(u64::from(broke_t))),
    };
    let broke_clause = if broke_t == 0 {
        format!("mai sotto {broke}")
    } else {
        format!("prima volta sotto {broke} al round {}", int(u64::from(broke_t)))
    };
    let fin = eur(lat.at(game.rounds, s.get(FINAL_K, i)));
    let detail = match p.role {
        Role::BiggestFall => {
            let (a, ak) = (s.get(FALL_FROM_T, i), s.get(FALL_FROM_K, i));
            let (b, bk) = (s.get(FALL_TO_T, i), s.get(FALL_TO_K, i));
            format!(
                "#{} · da {} al round {} a {} al round {} · fine {} · {}",
                int(p.id),
                eur(lat.at(a, ak)),
                int(u64::from(a)),
                eur(lat.at(b, bk)),
                int(u64::from(b)),
                fin,
                broke_clause
            )
        }
        _ => {
            let (pt, pk) = (s.get(PEAK_T, i), s.get(PEAK_K, i));
            let note = if p.role == Role::RichThenBroke {
                format!(" · tra chi ha toccato {rich}, il più in basso alla fine")
            } else {
                String::new()
            };
            format!(
                "#{} · picco {} al round {} · fine {} · {}{}",
                int(p.id),
                eur(lat.at(pt, pk)),
                int(u64::from(pt)),
                fin,
                broke_clause,
                note
            )
        }
    };
    (name, detail)
}

/// The headline figures: (value, label).
pub fn tiles(game: &Game, st: &Stats) -> Vec<(String, String)> {
    let (rich, broke) = (thr(game.rich), thr(game.broke));
    let p = st.players;
    let verb = if st.rich_ever == 1 { "ha" } else { "hanno" };
    vec![
        (int(st.rich_ever), format!("{verb} toccato {rich} almeno una volta ({} dei giocatori)", pct(st.rich_ever, p))),
        if st.rich_ever > 0 {
            (
                format!("{} di {}", int(st.rich_ever_broke_at_end), int(st.rich_ever)),
                format!("di chi ha toccato {rich}: sotto {broke} alla fine"),
            )
        } else {
            ("–".into(), format!("nessuno ha toccato {rich}, quindi nessuno da contare qui"))
        },
        if st.rich_most_at_once > 0 {
            (
                int(st.rich_most_at_once),
                format!(
                    "massimo di giocatori sopra {rich} nello stesso round (la prima volta al round {})",
                    int(u64::from(st.rich_most_at_once_round))
                ),
            )
        } else {
            ("0".into(), format!("in nessun round c'è un giocatore sopra {rich}"))
        },
        (int(st.never_broke), format!("mai sotto {broke} ({} dei giocatori)", pct(st.never_broke, p))),
        (
            pct(st.above_start_at_end, p),
            format!("sopra i {} iniziali alla fine ({})", eur(game.start.log10()), giocatori(st.above_start_at_end)),
        ),
    ]
}

/// Legend text for the thin orange lines: (clipped view, full paths view, checkbox label).
pub fn rich_texts(game: &Game, st: &Stats, shown: usize) -> (String, String, String) {
    let rich = thr(game.rich);
    if st.rich_ever == 0 {
        let none = format!("Nessun giocatore ha toccato {rich}.");
        return (none.clone(), none, String::new());
    }
    let overlap = "Giocatori allo stesso livello nello stesso round hanno la stessa ricchezza e le loro linee si sovrappongono: il numero esatto per round è nella striscia sotto il grafico.";
    let capped = if (shown as u64) < st.rich_ever {
        format!(" Disegnati {} su {} (quelli con il numero più basso).", int(shown as u64), int(st.rich_ever))
    } else {
        String::new()
    };
    let who = if st.rich_ever == 1 {
        format!("l'unico giocatore che ha toccato {rich}")
    } else {
        format!("i {} giocatori che hanno toccato {rich}", int(st.rich_ever))
    };
    let clipped = format!("Arancione sottile: {who}, solo mentre sono sopra la soglia. {overlap}{capped}");
    let full = format!("Arancione sottile: {who}, percorso completo. {overlap}{capped}");
    let toggle = format!("Mostra i percorsi completi ({})", giocatori(shown as u64));
    (clipped, full, toggle)
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
        .str(&format!("{} × {} round · seed {}", giocatori(p), int(u64::from(game.rounds)), game.seed))
        .raw(",");

    j.key("tiles").raw("[");
    for (n, (v, l)) in tiles(game, st).iter().enumerate() {
        if n > 0 {
            j.raw(",");
        }
        j.raw("{").key("value").str(v).raw(",").key("label").str(l).raw("}");
    }
    j.raw("],");

    j.key("players").raw("[");
    for (n, pk) in picks.iter().enumerate() {
        let (name, detail) = player_text(game, s, pk);
        if n > 0 {
            j.raw(",");
        }
        j.raw("{").key("slot").num(pk.role.slot() as f64).raw(",");
        j.key("name").str(&name).raw(",").key("detail").str(&detail).raw("}");
    }
    j.raw("],");

    let (clipped, full, toggle) = rich_texts(game, st, rich_shown);
    j.key("rich").raw("{").key("count").num(st.rich_ever as f64).raw(",");
    j.key("note").str(&clipped).raw(",").key("noteFull").str(&full).raw(",").key("toggle").str(&toggle).raw("},");

    j.key("lines").raw("[");
    j.raw("{").key("style").str("solid").raw(",").key("label").str("Giocatore mediano").raw("},");
    j.raw("{").key("style").str("dot").raw(",").key("label").str("Media di tutti i giocatori").raw("},");
    j.raw("{").key("style").str("dash").raw(",");
    j.key("label").str(&format!("Valore atteso (+{}% a round)", percent_step(game))).raw("},");
    j.raw("{").key("style").str("bar").raw(",");
    j.key("label").str(&format!("Striscia: giocatori sopra {rich} in ogni round")).raw("}");
    j.raw("],");

    j.key("scale").raw("{");
    j.key("title").str("Giocatori nella stessa cella (round × livello di ricchezza), scala logaritmica").raw(",");
    j.key("ticks").raw("[");
    for (n, (v, pos)) in legend_ticks(e.max_cell).iter().enumerate() {
        if n > 0 {
            j.raw(",");
        }
        j.raw("{").key("label").str(&int(u64::from(*v))).raw(",").key("pos").num(*pos).raw("}");
    }
    j.raw("]},");

    j.key("table").raw("{").key("caption").str("Valori per round (ogni 10% dei round)").raw(",");
    let head = [
        "Round".to_string(),
        "Giocatore mediano".into(),
        "Media".into(),
        "Valore atteso".into(),
        format!("Sopra {rich}"),
        format!("Sotto {broke}"),
    ];
    j.key("head").strs(&head).raw(",").key("rows").raw("[");
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
        let row = [
            int(u64::from(t)),
            eur(e.median[i]),
            eur(e.mean[i]),
            eur(e.expected[i]),
            int(e.rich_now[i]),
            int(e.broke_now[i]),
        ];
        j.strs(&row);
    }
    j.raw("]},");

    j.key("aria").str(&format!(
        "Ricchezza di {} in {} round, scala logaritmica. Alla fine il giocatore mediano ha {}, la media è {}, il valore atteso {}. Sotto il grafico, il numero di giocatori sopra {rich} in ogni round.",
        giocatori(p),
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

/// Tooltip for a probed pixel (see [`crate::probe`]).
pub fn hover_json(
    game: &Game,
    e: &Ensemble,
    lay: &Layout,
    highlighted: &[(u64, usize, Vec<f64>)],
    p: &Probe,
) -> String {
    let t = p.round as usize;
    let mut j = Json::new();
    j.raw("{").key("inside").raw("true,").key("x").num(f64::from(lay.x_of(f64::from(p.round)))).raw(",");
    j.key("top").num(f64::from(lay.y)).raw(",").key("bottom").num(f64::from(lay.strip_y + lay.strip_h)).raw(",");
    let head = if p.shared.0 != p.shared.1 {
        format!(
            "Round {} (questo pixel copre i round {}–{})",
            int(t as u64),
            int(u64::from(p.shared.0)),
            int(u64::from(p.shared.1))
        )
    } else {
        format!("Round {}", int(t as u64))
    };
    j.key("head").str(&head).raw(",");
    j.key("players").raw("[");
    for (n, (id, slot, path)) in highlighted.iter().enumerate() {
        if n > 0 {
            j.raw(",");
        }
        j.raw("{").key("slot").num(*slot as f64).raw(",").key("label").str(&format!("#{}", int(*id))).raw(",");
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
    match p.cell {
        Some((l, n)) => {
            j.raw("{").key("label").str(&format!("Giocatori a {} in questo round", eur(l))).raw(",");
            j.key("value").str(&int(u64::from(n))).raw("}");
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
    use coin_core::{ensemble::ensemble, sim::simulate, stats::stats};

    #[test]
    fn json_strings_are_escaped() {
        let mut j = Json::new();
        j.str("a\"b\\c\nd\u{1}");
        assert_eq!(j.finish(), r#""a\"b\\c\nd\u0001""#);
    }

    #[test]
    fn lede_states_the_rules() {
        let l = lede(&Game::peters(1000, 1));
        assert!(l.contains("sale del 50%") && l.contains("scende del 40%") && l.contains("100 €"));
        assert!(l.contains("del 5% a round"));
    }

    fn st(rich_ever: u64, broke_end: u64, most: u64, never: u64, up: u64) -> Stats {
        Stats {
            players: 10_000,
            rich_ever,
            rich_ever_broke_at_end: broke_end,
            rich_most_at_once: most,
            rich_most_at_once_round: 239,
            rich_at_end: 0,
            never_broke: never,
            above_start_at_end: up,
        }
    }

    #[test]
    fn singular_and_plural_agree() {
        let g = Game::peters(1000, 1);
        let one = tiles(&g, &st(1, 1, 1, 1, 1));
        assert!(one[0].1.starts_with("ha toccato 1 miliardo €"), "{}", one[0].1);
        assert!(one[4].1.ends_with("(1 giocatore)"), "{}", one[4].1);
        let many = tiles(&g, &st(277, 243, 43, 72, 154));
        assert!(many[0].1.starts_with("hanno toccato 1 miliardo €"));
        assert!(many[4].1.ends_with("(154 giocatori)"));
        assert_eq!(altri(1), "un altro giocatore");
        assert_eq!(altri(957), "altri 957 giocatori");
        let (clip1, _, toggle1) = rich_texts(&g, &st(1, 1, 1, 1, 1), 1);
        assert!(clip1.contains("l'unico giocatore") && toggle1.ends_with("(1 giocatore)"));
        let (clip, full, toggle) = rich_texts(&g, &st(277, 243, 43, 72, 154), 277);
        assert!(clip.contains("i 277 giocatori") && clip.contains("solo mentre") && full.contains("percorso completo"));
        assert!(toggle.ends_with("(277 giocatori)"));
        let (none, _, t0) = rich_texts(&g, &st(0, 0, 0, 5, 5), 0);
        assert_eq!(none, "Nessun giocatore ha toccato 1 miliardo €.");
        assert!(t0.is_empty());
        assert_eq!(tiles(&g, &st(0, 0, 0, 5, 5))[1].0, "–");
    }

    #[test]
    fn player_texts_are_true_for_every_role_seen() {
        use coin_core::stats::picks;
        let mut seen = std::collections::HashSet::new();
        for seed in 0..40u64 {
            for (game, players) in
                [(Game::peters(100, seed), 1000), (Game { rich: 1e4, ..Game::peters(30, seed) }, 2000)]
            {
                let (counts, s) = simulate(&game, players);
                let e = ensemble(&game, players as u64, &counts);
                let stt = stats(&game, &s, &e);
                let lat = game.lattice();
                let ps = picks(&game, &s);
                for p in &ps {
                    seen.insert(format!("{:?}", p.role));
                    let (name, detail) = player_text(&game, &s, p);
                    let i = p.id as usize;
                    assert!(detail.starts_with(&format!("#{} ·", int(p.id))));
                    assert_eq!(name.contains("a pari merito") || name.contains("insieme ad"), p.ties > 0, "{name}");
                    if p.role == Role::BiggestFall {
                        let from = eur(lat.at(s.get(FALL_FROM_T, i), s.get(FALL_FROM_K, i)));
                        assert!(detail.contains(&format!("da {from}")), "{detail}");
                    }
                    if s.get(BROKE_T, i) == 0 {
                        assert!(detail.contains("mai sotto 1 €"));
                    }
                }
                let v = view_json(&game, &s, &e, &stt, &ps, 0, 1.0, 1);
                assert!(v.starts_with('{') && v.ends_with('}'));
            }
        }
        assert!(seen.len() >= 4, "roles seen: {seen:?}");
    }
}

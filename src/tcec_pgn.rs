use crate::tcec::EngineName;
use anyhow::{bail, Result};
use pgn_reader::{BufferedReader, RawComment, RawHeader, SanPlus, Skip, Visitor};
use std::hash::{Hash, Hasher};

const EVENT_KEY: &str = "Event";
const WHITE_HEADER_KEY: &str = "White";
const BLACK_HEADER_KEY: &str = "Black";
const DATE_HEADER_KEY: &str = "Date";
const BOOK_MOVE_COMMENT_PREFIX: &str = "book,";

#[derive(Debug, Clone)]
pub struct PgnMove {
    notation: String,
    in_book: bool,
}

#[derive(Debug, Clone)]
pub struct Pgn {
    pub white_player: EngineName,
    pub black_player: EngineName,
    pub date: String,
    pub event: String,

    pub moves: Vec<PgnMove>,
}

impl Pgn {
    // There's a strange issue where moves later in the game can be reported as 'book' moves.
    // It seems to happen for tablebase moves or other moves with no UCI info.
    // To ensure we get only the opening, stop once we hit the first non-book move.
    fn opening(&self) -> impl Iterator<Item = &PgnMove> {
        self.moves.iter().take_while(|mv| mv.in_book)
    }

    /// The game is 'out of book' if any of the moves that were played are not book moves
    pub fn out_of_book(&self) -> bool {
        self.moves.iter().any(|mv| !mv.in_book)
    }

    pub fn has_player(&self, player: &str) -> bool {
        self.white_player_is(player) || self.black_player_is(player)
    }

    fn white_player_is(&self, player: &str) -> bool {
        self.white_player.matches(player)
    }

    fn black_player_is(&self, player: &str) -> bool {
        self.black_player.matches(player)
    }

    pub fn as_hash(&self) -> u64 {
        let mut hasher = std::hash::DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

// The hash of a TCEC PGN is the hash of the players, the date, and the book.
// That is to say, we consider games equivalent if they are played by the same players
// on the same day, with the same opening book.
// FIXME: This doesn't account for replays.
impl Hash for Pgn {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.white_player.hash(state);
        self.black_player.hash(state);
        self.date.hash(state);

        for mv in self.opening() {
            mv.notation.hash(state);
        }
    }
}

impl PartialEq<Self> for Pgn {
    fn eq(&self, other: &Self) -> bool {
        self.as_hash() == other.as_hash()
    }
}

impl Eq for Pgn {}

struct PgnInfoBuilder {
    pub white_player: Option<String>,
    pub black_player: Option<String>,
    pub date: Option<String>,
    pub event: Option<String>,

    pub moves: Vec<PgnMove>,

    pub last_san: Option<String>,
}

impl PgnInfoBuilder {
    pub fn new() -> PgnInfoBuilder {
        Self {
            white_player: None,
            black_player: None,
            date: None,
            event: None,
            moves: vec![],

            last_san: None,
        }
    }
}

impl Visitor for PgnInfoBuilder {
    type Result = Pgn;

    fn header(&mut self, key: &[u8], value: RawHeader<'_>) {
        let key = String::from_utf8_lossy(key);
        let value = value.decode_utf8_lossy();

        if key == EVENT_KEY {
            self.event = Some(value.to_string());
        }

        if key == WHITE_HEADER_KEY {
            self.white_player = Some(value.to_string());
        }

        if key == BLACK_HEADER_KEY {
            self.black_player = Some(value.to_string());
        }

        if key == DATE_HEADER_KEY {
            self.date = Some(value.to_string());
        }
    }

    fn san(&mut self, san: SanPlus) {
        assert_eq!(self.last_san, None);

        self.last_san = Some(san.to_string());
    }

    fn comment(&mut self, comment: RawComment<'_>) {
        let Some(san) = self.last_san.clone() else {
            // We may have comments with no preceding SAN
            return;
        };

        let comment = String::from_utf8_lossy(comment.as_bytes()).to_string();
        let is_book_move = comment.starts_with(BOOK_MOVE_COMMENT_PREFIX);

        self.moves.push(PgnMove {
            notation: san,
            in_book: is_book_move,
        });

        self.last_san = None;
    }

    fn begin_variation(&mut self) -> Skip {
        Skip(true)
    }

    fn end_game(&mut self) -> Self::Result {
        assert_ne!(self.white_player, None);
        assert_ne!(self.black_player, None);
        assert_ne!(self.date, None);
        assert_ne!(self.event, None);

        Pgn {
            white_player: EngineName::new(&self.white_player.clone().unwrap()),
            black_player: EngineName::new(&self.black_player.clone().unwrap()),
            date: self.date.clone().unwrap(),
            event: self.event.clone().unwrap(),
            moves: self.moves.clone(),
        }
    }
}

pub fn get_pgn_info(pgn: &str) -> Result<Pgn> {
    let mut reader = BufferedReader::new_cursor(pgn);

    let pgn_info = reader.read_game(&mut PgnInfoBuilder::new())?;

    let Some(pgn_info) = pgn_info else {
        bail!("Empty PGN")
    };

    Ok(pgn_info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pgn_parsing_grabs_correct_information() {
        let sample_pgn = r#"[Event "TCEC Season 29 - Category 1 Playoff"]
[Site "https://tcec-chess.com"]
[Date "2025.12.02"]
[Round "2.1"]
[White "c4ke 1.1"]
[Black "Minic 3.44"]
[Result "*"]
[BlackElo "3436"]
[ECO "B43"]
[GameStartTime "2025-12-02T13:20:38.758 UTC"]
[Opening "Sicilian"]
[Termination "unterminated"]
[TimeControl "1800+3"]
[Variation "Kan, 5.Nc3"]
[WhiteElo "3183"]

{WhiteEngineOptions: Protocol=uci; Threads=256; Hash=262144;, BlackEngineOptions: Protocol=uci; Threads=512; Hash=256000; PawnHash=2048; NNUEFile=embedded; CommandLineOptions=-uci -syzygyPath /home/syzygy7;}
1. e4 {book, mb=+0+0+0+0+0,} c5 {book, mb=+0+0+0+0+0,}
2. Nf3 {book, mb=+0+0+0+0+0,} e6 {book, mb=+0+0+0+0+0,}
3. d4 {book, mb=+0+0+0+0+0,} cxd4 {book, mb=-1+0+0+0+0,}
4. Nxd4 {book, mb=+0+0+0+0+0,} a6 {book, mb=+0+0+0+0+0,}
5. Nc3 {book, mb=+0+0+0+0+0,} Qc7 {book, mb=+0+0+0+0+0,}
6. g3 {book, mb=+0+0+0+0+0,} b5 {book, mb=+0+0+0+0+0,}
7. Bg2 {d=32, sd=32, mt=96132, tl=1706868, s=0, n=0, pv=Bg2, tb=null, h=0.0, ph=0.0, wv=0.74, R50=49, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
Nc6 {d=33, sd=52, mt=126033, tl=1676967, s=192424652, n=24238964382, pv=Bb7 O-O Be7 Re1 d6 a4 b4 Na2 Nf6 Nxb4 O-O Nd3 Nbd7 Bd2 Rac8 Qe2 Ne5 a5 Rfd8 Bc3 Nc4 Bb4 Nd7 c3 Bf8 Red1 Re8 Nb3 Nde5 Nxe5, tb=1, h=36.6, ph=0.0, wv=0.88, R50=49, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
8. O-O {d=34, sd=34, mt=108197, tl=1601671, s=0, n=0, pv=O-O, tb=null, h=0.0, ph=0.0, wv=0.68, R50=48, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
Nxd4 {d=35, sd=53, pd=O-O, mt=150055, tl=1529912, s=197324099, n=29601378978, pv=Nxd4 Qxd4 f6 Be3 Ne7 a4 b4 Qxb4 Rb8 Qd4 Nc6 Qd1 h5 b3 h4 Ne2 g5 Nc1 Bb4 Rb1 a5 Qe2 d6 h3 Bc5 Nd3 Bxe3 Qxe3 hxg3 fxg3 O-O Rfc1 Ne5 Nxe5, tb=10, h=71.1, ph=100.0, wv=0.92, R50=50, Rd=-9, Rr=-1000, mb=+0-1+0+0+0,}
9. Qxd4 {d=35, sd=35, mt=100682, tl=1503989, s=0, n=0, pv=Qxd4, tb=null, h=0.0, ph=0.0, wv=1.14, R50=50, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
f6 {d=33, sd=48, pd=Qxd4, mt=53840, tl=1479072, s=208728575, n=11231475551, pv=Ne7 a4 b4 Qxb4 Rb8 Qd4 Nc6 Qc4 Rb4 Qe2 Be7 Re1 Bf6 Bf4 d6 Nd1 Bxb2 Nxb2 Rxb2 Bc1 Rb6 Be3 Rb8 Red1 Na5 Bf1 O-O Qd3 Rd8 Rab1 h5 Rdc1 Rxb1 Rxb1, tb=4, h=83.5, ph=100.0, wv=1.10, R50=50, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
10. Rd1 {d=34, sd=34, mt=83264, tl=1423725, s=0, n=0, pv=Rd1, tb=null, h=0.0, ph=0.0, wv=0.54, R50=49, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
Bb7 {d=36, sd=51, pd=Be3, mt=117012, tl=1365060, s=201592485, n=23581280709, pv=Bb7 Be3 Ne7 a4 Nc6 Qd2 b4 Na2 Ne5 Bf4 a5 c3 bxc3 Nxc3 Bb4 Nb5 Qb6 Be3 Qd8 Qe2 O-O f4 Nf7 Qd3 d6 Rdc1 Rc8 Rxc8 Bxc8 Rc1 Bd7 Qb3 g5 fxg5 fxg5 Nc7 Bc5 Bxc5 Qxc7, tb=20, h=89.9, ph=66.6, wv=0.67, R50=49, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
11. Be3 {d=36, sd=36, mt=106534, tl=1320191, s=0, n=0, pv=Be3, tb=null, h=0.0, ph=0.0, wv=1.01, R50=48, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
Ne7 {d=36, sd=61, pd=Be3, mt=144513, tl=1223547, s=200444379, n=28958802352, pv=Ne7 a4 Nc6 Qd2 b4 Na2 Be7 Bf4 Qd8 Bd6 Bxd6 Qxd6 Qe7 Qc7 d6 Qxd6 Rd8 Qxe7+ Kxe7 e5 a5 exf6+ gxf6 b3 Rxd1+ Rxd1 Rc8 h3 Rc7 c3 bxc3 Nxc3 Ne5 Rc1 Bxg2 Kxg2 Rc5 Ne2 Rxc1 Nxc1 Kd6 f4 Nd7 Kf3 Kc5 f5 exf5 Kf4 Kd4 Kxf5 Kc3 Ke6 Nc5+ Kxf6 Ne4+, tb=150, h=95.2, ph=75.0, wv=0.84, R50=48, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
12. Qd2 {d=36, sd=36, mt=84680, tl=1238511, s=0, n=0, pv=Qd2, tb=null, h=0.0, ph=0.0, wv=1.08, R50=47, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
Nc6 {d=35, sd=57, pd=Qd2, mt=115271, tl=1111276, s=202816783, n=23370374394, pv=Nc6 a4 b4 Na2 Be7 b3 Rd8 Rac1 Qa5 c3 bxc3 Rxc3 Ba8 Qc2 O-O Rcd3 Rc8 Rxd7 Ne5 Qd2 Qxd2 R7xd2 a5 Bd4 Nd7 Rc1 Ba3 Rb1 Nc5 e5 fxe5 Bxe5 Be4 Bxe4 Nxe4 Re2 Bc5 Rxe4 Rxf2, tb=255, h=97.1, ph=80.0, wv=0.66, R50=47, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
13. a4 {d=36, sd=36, mt=63665, tl=1177846, s=0, n=0, pv=a4, tb=null, h=0.0, ph=0.0, wv=1.44, R50=50, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
b4 {d=35, sd=58, pd=a4, mt=90024, tl=1024252, s=205445730, n=18490115719, pv=b4 Na2 Be7 Bf4 Qd8 Bd6 Bxd6 Qxd6 Qe7 Qc7 d6 Qxd6 Rd8 Qxe7+ Kxe7 e5 a5 exf6+ gxf6 b3 Rxd1+ Rxd1 Rc8 Nc1 Nd8 Rd2 Bxg2 Kxg2 Nf7 f3 h5 h3 e5 Re2 Ke6 Nd3 Kf5 g4+ Kg5 Nf2 hxg4 hxg4 f5 gxf5 Kxf5 Nd3 Rc3, tb=194, h=98.6, ph=83.3, wv=0.85, R50=50, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
14. Na2 {d=34, sd=34, mt=80976, tl=1099870, s=0, n=0, pv=Na2, tb=null, h=0.0, ph=0.0, wv=0.87, R50=49, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
Be7 {d=32, sd=59, pd=Na2, mt=60913, tl=966339, s=210473095, n=12813180747, pv=Be7 Bf4 Qd8 Bd6 Bxd6 Qxd6 Qe7 Qc7 d6 Qxd6 Rd8 Qxe7+ Kxe7 e5 a5 exf6+ gxf6 b3 Rxd1+ Rxd1 Rc8 Nc1 Nd8 Rd2 Bxg2 Kxg2 Nf7 Nd3 Nd6 Nf4 Rc6 Kf3 e5 Nd5+ Ke6 Ne3 Rc5 g4 Rc3 Kg2, tb=202, h=98.9, ph=85.7, wv=0.76, R50=49, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
15. Bf4 {d=37, sd=37, mt=62145, tl=1040725, s=0, n=0, pv=Bf4, tb=null, h=0.0, ph=0.0, wv=1.45, R50=48, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
Qd8 {d=36, sd=56, pd=Bf4, mt=128000, tl=841339, s=208638123, n=20096858070, pv=Qd8 Bd6 a5 Rac1 Bxd6 f4 Qe7 Rb1 Rc8 Qxd6 Qxd6 Rxd6 Ke7 c3 Kxd6 cxb4, tb=2669, h=99.2, ph=87.5, wv=0.74, R50=48, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
16. Bd6 {d=39, sd=39, mt=54745, tl=988980, s=0, n=0, pv=Bd6, tb=null, h=0.0, ph=0.0, wv=1.47, R50=47, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
Bxd6 {d=38, sd=59, pd=Bd6, mt=93051, tl=751288, s=209291026, n=19466995541, pv=Bxd6 Qxd6 Qe7 Qc7 d6 Qxd6 Rd8 Qxe7+ Kxe7 e5 Rb8 exf6+ gxf6 Rd2 Ne5 Re1 Nf3+ Bxf3 Bxf3 Re3 Bc6 b3 Rhd8 Rxd8 Rxd8 Kf1 a5 Ke1 e5 Nc1 Bd5 Nd3 h5 Nb2 Rg8 f3 Rc8 Kd2 Ke6 f4 e4 Nc4 Bxc4 Rxe4+ Kd5 Rxc4 Rxc4 bxc4+, tb=24526, h=99.6, ph=88.8, wv=0.88, R50=50, Rd=-9, Rr=-1000, mb=+0+0-1+0+0,}
17. Qxd6 {d=42, sd=42, mt=79375, tl=912605, s=0, n=0, pv=Qxd6, tb=null, h=0.0, ph=0.0, wv=1.38, R50=50, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
Qe7 {d=35, sd=55, pd=Qxd6, mt=41546, tl=712742, s=232587351, n=9656562004, pv=Qe7 Qc7 d6 Qxd6 Rd8 Qxe7+ Kxe7 e5 Rb8 exf6+ gxf6 f4 Rhd8 Rxd8 Nxd8 Rd1 Bxg2 Kxg2 b3 cxb3 Rxb3 Rd2 Nb7 Nc1 Rb4 b3 e5 fxe5 fxe5 Kf2 Na5 Rd3 Ke6 Rc3 Kd6 h3 Kd5 Rd3+ Ke6 Re3 Kd6, tb=19595, h=99.7, ph=90.0, wv=0.76, R50=49, Rd=-9, Rr=-1000, mb=+0+0+0+0+0,}
*
"#;

        let pgn_info = get_pgn_info(sample_pgn).unwrap();

        assert!(pgn_info.white_player.matches("c4ke"));
        assert!(pgn_info.black_player.matches("Minic"));
        assert_eq!(pgn_info.date, "2025.12.02");
        assert_eq!(pgn_info.event, "TCEC Season 29 - Category 1 Playoff");
        assert!(pgn_info.out_of_book())
    }

    #[test]
    fn test_pgn_parsing_in_book_returns_true() {
        let sample_pgn = r#"[Event "TCEC Season 29 - Category 1 Playoff"]
[Site "https://tcec-chess.com"]
[Date "2025.12.02"]
[Round "2.1"]
[White "c4ke 1.1"]
[Black "Minic 3.44"]
[Result "*"]
[BlackElo "3436"]
[ECO "B43"]
[GameStartTime "2025-12-02T13:20:38.758 UTC"]
[Opening "Sicilian"]
[Termination "unterminated"]
[TimeControl "1800+3"]
[Variation "Kan, 5.Nc3"]
[WhiteElo "3183"]

{WhiteEngineOptions: Protocol=uci; Threads=256; Hash=262144;, BlackEngineOptions: Protocol=uci; Threads=512; Hash=256000; PawnHash=2048; NNUEFile=embedded; CommandLineOptions=-uci -syzygyPath /home/syzygy7;}
1. e4 {book, mb=+0+0+0+0+0,} c5 {book, mb=+0+0+0+0+0,}
2. Nf3 {book, mb=+0+0+0+0+0,} e6 {book, mb=+0+0+0+0+0,}
3. d4 {book, mb=+0+0+0+0+0,} cxd4 {book, mb=-1+0+0+0+0,}
4. Nxd4 {book, mb=+0+0+0+0+0,} a6 {book, mb=+0+0+0+0+0,}
5. Nc3 {book, mb=+0+0+0+0+0,} Qc7 {book, mb=+0+0+0+0+0,}
*
"#;

        let pgn_info = get_pgn_info(sample_pgn).unwrap();
        assert!(!pgn_info.out_of_book())
    }
}

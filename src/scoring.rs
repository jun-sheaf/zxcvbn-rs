use std::collections::HashMap;
use std::cmp;
use super::matching::Match;

#[derive(Debug, Clone)]
#[doc(hidden)]
pub struct GuessCalculation {
    /// Estimated guesses needed to crack the password
    pub guesses: u64,
    /// Order of magnitude of `guesses`
    pub guesses_log10: u16,
    /// The list of patterns the guess calculation was based on
    pub sequence: Vec<Match>,
}

#[derive(Debug, Clone)]
struct Optimal {
    /// optimal.m[k][l] holds final match in the best length-l match sequence covering the
    /// password prefix up to k, inclusive.
    /// if there is no length-l sequence that scores better (fewer guesses) than
    /// a shorter match sequence spanning the same prefix, optimal.m[k][l] is undefined.
    m: Vec<HashMap<usize, Match>>,
    /// same structure as optimal.m -- holds the product term Prod(m.guesses for m in sequence).
    /// optimal.pi allows for fast (non-looping) updates to the minimization function.
    pi: Vec<HashMap<usize, u64>>,
    /// same structure as optimal.m -- holds the overall metric.
    g: Vec<HashMap<usize, u64>>,
}

#[doc(hidden)]
pub const REFERENCE_YEAR: i16 = 2000;
const MIN_YEAR_SPACE: i16 = 20;
const BRUTEFORCE_CARDINALITY: u64 = 10;
const MIN_GUESSES_BEFORE_GROWING_SEQUENCE: u64 = 10000;
const MIN_SUBMATCH_GUESSES_SINGLE_CHAR: u64 = 10;
const MIN_SUBMATCH_GUESSES_MULTI_CHAR: u64 = 50;

#[doc(hidden)]
pub fn most_guessable_match_sequence(password: &str,
                                     matches: &[super::matching::Match],
                                     exclude_additive: bool)
                                     -> GuessCalculation {
    let n = password.len();

    // partition matches into sublists according to ending index j
    let mut matches_by_j: Vec<Vec<Match>> = (0..n).map(|_| Vec::new()).collect();
    for m in matches {
        matches_by_j[m.j].push(m.clone());
    }
    // small detail: for deterministic output, sort each sublist by i.
    for lst in &mut matches_by_j {
        lst.sort_by_key(|m| m.i);
    }

    let mut optimal = Optimal {
        m: (0..n).map(|_| HashMap::new()).collect(),
        pi: (0..n).map(|_| HashMap::new()).collect(),
        g: (0..n).map(|_| HashMap::new()).collect(),
    };

    /// helper: considers whether a length-l sequence ending at match m is better (fewer guesses)
    /// than previously encountered sequences, updating state if so.
    fn update(mut m: Match,
              l: usize,
              password: &str,
              optimal: &mut Optimal,
              exclude_additive: bool) {
        let k = m.j;
        let mut pi = estimate_guesses(&mut m, password);
        if l > 1 {
            // we're considering a length-l sequence ending with match m:
            // obtain the product term in the minimization function by multiplying m's guesses
            // by the product of the length-(l-1) sequence ending just before m, at m.i - 1.
            pi *= optimal.pi[m.i - 1][&(l - 1)];
        }
        // calculate the minimization func
        let mut g = factorial(l) as u64 * pi;
        if !exclude_additive {
            g += MIN_GUESSES_BEFORE_GROWING_SEQUENCE.pow((l - 1) as u32);
        }
        // update state if new best.
        // first see if any competing sequences covering this prefix, with l or fewer matches,
        // fare better than this sequence. if so, skip it and return.
        for (&competing_l, &competing_g) in &optimal.g[k] {
            if competing_l > l {
                continue;
            }
            if competing_g <= g as u64 {
                return;
            }
        }
        // this sequence might be part of the final optimal sequence.
        *optimal.g[k].entry(l).or_insert(0) = g as u64;
        *optimal.m[k].entry(l).or_insert_with(Match::default) = m;
        *optimal.pi[k].entry(l).or_insert(0) = pi;
    }

    /// helper: evaluate bruteforce matches ending at k.
    fn bruteforce_update(k: usize, password: &str, optimal: &mut Optimal, exclude_additive: bool) {
        // see if a single bruteforce match spanning the k-prefix is optimal.
        let m = make_bruteforce_match(0, k, password);
        update(m, 1, password, optimal, exclude_additive);
        for i in 1..(k + 1) {
            // generate k bruteforce matches, spanning from (i=1, j=k) up to (i=k, j=k).
            // see if adding these new matches to any of the sequences in optimal[i-1]
            // leads to new bests.
            let m = make_bruteforce_match(i, k, password);
            for (l, last_m) in optimal.m[i - 1].clone() {
                // corner: an optimal sequence will never have two adjacent bruteforce matches.
                // it is strictly better to have a single bruteforce match spanning the same region:
                // same contribution to the guess product with a lower length.
                // --> safe to skip those cases.
                if last_m.pattern == "bruteforce" {
                    continue;
                }
                // try adding m to this length-l sequence.
                update(m.clone(), l + 1, password, optimal, exclude_additive);
            }
        }
    }

    /// helper: make bruteforce match objects spanning i to j, inclusive.
    fn make_bruteforce_match(i: usize, j: usize, password: &str) -> Match {
        Match::default()
            .pattern("bruteforce")
            .token(password[i..(j + 1)].to_string())
            .i(i)
            .j(j)
            .build()
    }

    /// helper: step backwards through optimal.m starting at the end,
    /// constructing the final optimal match sequence.
    #[allow(many_single_char_names)]
    fn unwind(n: usize, optimal: &mut Optimal) -> Vec<Match> {
        let mut optimal_match_sequence = Vec::new();
        let mut k = n - 1;
        // find the final best sequence length and score
        let mut l = None;
        let mut g = None;
        for (candidate_l, candidate_g) in &optimal.g[k] {
            if g.is_none() || *candidate_g < *g.as_ref().unwrap() {
                l = Some(*candidate_l);
                g = Some(*candidate_g);
            }
        }

        loop {
            let m = &optimal.m[k][&l.unwrap()];
            optimal_match_sequence.insert(0, m.clone());
            if m.i == 0 {
                break;
            }
            k = m.i - 1;
            l = l.map(|x| x - 1);
        }
        optimal_match_sequence
    }

    for (k, match_by_j) in matches_by_j.iter().enumerate() {
        for m in match_by_j {
            if m.i > 0 {
                let keys: Vec<usize> = optimal.m[m.i - 1].keys().cloned().collect();
                for l in keys {
                    update(m.clone(), l + 1, password, &mut optimal, exclude_additive);
                }
            } else {
                update(m.clone(), 1, password, &mut optimal, exclude_additive);
            }
        }
        bruteforce_update(k, password, &mut optimal, exclude_additive);
    }
    let optimal_match_sequence = unwind(n, &mut optimal);
    let optimal_l = optimal_match_sequence.len();

    // corner: empty password
    let guesses = if password.is_empty() {
        1
    } else {
        optimal.g[n - 1][&optimal_l]
    };

    GuessCalculation {
        guesses: guesses as u64,
        guesses_log10: (guesses as f64).log10() as u16,
        sequence: optimal_match_sequence,
    }
}

fn factorial(n: usize) -> usize {
    // unoptimized, called only on small n
    if n < 2 {
        return 1;
    }
    (2..(n + 1)).fold(1, |acc, x| acc * x)
}

fn estimate_guesses(m: &mut Match, password: &str) -> u64 {
    if let Some(guesses) = m.guesses {
        // a match's guess estimate doesn't change. cache it.
        return guesses;
    }
    let min_guesses = if m.token.len() < password.len() {
        if m.token.len() == 1 {
            MIN_SUBMATCH_GUESSES_SINGLE_CHAR
        } else {
            MIN_SUBMATCH_GUESSES_MULTI_CHAR
        }
    } else {
        1
    };
    let guesses = ESTIMATION_FUNCTIONS.iter().find(|x| x.0 == m.pattern).unwrap().1.estimate(m);
    m.guesses = Some(cmp::max(guesses, min_guesses));
    m.guesses.unwrap()
}

lazy_static! {
    static ref ESTIMATION_FUNCTIONS: [(&'static str, Box<Estimator>); 7] = [
        ("bruteforce", Box::new(BruteForceEstimator {})),
        ("dictionary", Box::new(DictionaryEstimator {})),
        ("spatial", Box::new(SpatialEstimator {})),
        ("repeat", Box::new(RepeatEstimator {})),
        ("sequence", Box::new(SequenceEstimator {})),
        ("regex", Box::new(RegexEstimator {})),
        ("date", Box::new(DateEstimator {})),
    ];
}

trait Estimator: Sync {
    fn estimate(&self, m: &mut Match) -> u64;
}

struct BruteForceEstimator {}

impl Estimator for BruteForceEstimator {
    fn estimate(&self, m: &mut Match) -> u64 {
        let guesses = BRUTEFORCE_CARDINALITY.pow(m.token.len() as u32);
        // small detail: make bruteforce matches at minimum one guess bigger than smallest allowed
        // submatch guesses, such that non-bruteforce submatches over the same [i..j] take precedence.
        let min_guesses = if m.token.len() == 1 {
            MIN_SUBMATCH_GUESSES_SINGLE_CHAR + 1
        } else {
            MIN_SUBMATCH_GUESSES_MULTI_CHAR + 1
        };
        cmp::max(guesses, min_guesses)
    }
}

struct DictionaryEstimator {}

impl Estimator for DictionaryEstimator {
    fn estimate(&self, m: &mut Match) -> u64 {
        m.base_guesses = m.rank.map(|x| x as u64);
        m.uppercase_variations = Some(uppercase_variations(m));
        m.l33t_variations = Some(l33t_variations(m));
        m.base_guesses.unwrap() * m.uppercase_variations.unwrap() * m.l33t_variations.unwrap() *
        if m.reversed { 2 } else { 1 }
    }
}

fn uppercase_variations(m: &Match) -> u64 {
    let word = &m.token;
    if word.chars().all(char::is_lowercase) || word.to_lowercase().as_str() == word {
        return 1;
    }
    // a capitalized word is the most common capitalization scheme,
    // so it only doubles the search space (uncapitalized + capitalized).
    // allcaps and end-capitalized are common enough too, underestimate as 2x factor to be safe.
    if word.chars().next().unwrap().is_uppercase() ||
       word.chars().last().unwrap().is_uppercase() || word.chars().all(char::is_uppercase) {
        return 2;
    }
    // otherwise calculate the number of ways to capitalize U+L uppercase+lowercase letters
    // with U uppercase letters or less. or, if there's more uppercase than lower (for eg. PASSwORD),
    // the number of ways to lowercase U+L letters with L lowercase letters or less.
    let upper = word.chars().filter(|c| c.is_uppercase()).count();
    let lower = word.chars().filter(|c| c.is_lowercase()).count();
    (1..(cmp::min(upper, lower) + 1)).map(|i| n_ck(upper + lower, i)).sum()
}

fn l33t_variations(m: &Match) -> u64 {
    if !m.l33t {
        return 1;
    }
    let mut variations = 1;
    for (subbed, unsubbed) in m.sub.as_ref().unwrap() {
        // lower-case match.token before calculating: capitalization shouldn't affect l33t calc.
        let token = m.token.to_lowercase();
        let subbed = token.chars().filter(|c| c == subbed).count();
        let unsubbed = token.chars().filter(|c| c == unsubbed).count();
        if subbed == 0 || unsubbed == 0 {
            // for this sub, password is either fully subbed (444) or fully unsubbed (aaa)
            // treat that as doubling the space (attacker needs to try fully subbed chars in addition to
            // unsubbed.)
            variations *= 2;
        } else {
            // this case is similar to capitalization:
            // with aa44a, U = 3, S = 2, attacker needs to try unsubbed + one sub + two subs
            let p = cmp::min(unsubbed, subbed);
            let possibilities: u64 = (1..(p + 1)).map(|i| n_ck(unsubbed + subbed, i)).sum();
            variations *= possibilities;
        }
    }
    variations as u64
}

fn n_ck(n: usize, k: usize) -> u64 {
    // http://blog.plover.com/math/choose.html
    (if k > n {
        0
    } else if k == 0 {
        1
    } else {
        let mut r: usize = 1;
        let mut n = n;
        for d in 1..(k + 1) {
            r = match r.checked_mul(n) {
                Some(res) => res,
                None => {
                    return ::std::u64::MAX;
                }
            };
            r /= d;
            n -= 1;
        }
        r
    }) as u64
}

struct SpatialEstimator {}

impl Estimator for SpatialEstimator {
    fn estimate(&self, m: &mut Match) -> u64 {
        #[allow(clone_on_copy)]
        let (starts, degree) = if ["qwerty", "dvorak"]
            .contains(&m.graph.as_ref().unwrap().as_str()) {
            (KEYBOARD_STARTING_POSITIONS.clone(), KEYBOARD_AVERAGE_DEGREE.clone())
        } else {
            (KEYPAD_STARTING_POSITIONS.clone(), KEYPAD_AVERAGE_DEGREE.clone())
        };
        let mut guesses = 0;
        let len = m.token.len();
        let turns = m.turns.unwrap();
        // estimate the number of possible patterns w/ length L or less with t turns or less.
        for i in 2..(len + 1) {
            let possible_turns = cmp::min(turns, i - 1);
            for j in 1..(possible_turns + 1) {
                guesses += n_ck(i - 1, j - 1) * starts as u64 * degree.pow(j as u32) as u64;
            }
        }
        // add extra guesses for shifted keys. (% instead of 5, A instead of a.)
        // math is similar to extra guesses of l33t substitutions in dictionary matches.
        if let Some(shifted_count) = m.shifted_count {
            let unshifted_count = len - shifted_count;
            if shifted_count == 0 || unshifted_count == 0 {
                guesses *= 2;
            } else {
                let shifted_variations = (1..(cmp::min(shifted_count, unshifted_count) + 1))
                    .into_iter()
                    .map(|i| n_ck(shifted_count + unshifted_count, i))
                    .sum();
                guesses *= shifted_variations;
            }
        }
        guesses
    }
}

lazy_static! {
    static ref KEYBOARD_AVERAGE_DEGREE: usize = calc_average_degree(&super::adjacency_graphs::QWERTY);
    // slightly different for keypad/mac keypad, but close enough
    static ref KEYPAD_AVERAGE_DEGREE: usize = calc_average_degree(&super::adjacency_graphs::KEYPAD);
    static ref KEYBOARD_STARTING_POSITIONS: usize = super::adjacency_graphs::QWERTY.len();
    static ref KEYPAD_STARTING_POSITIONS: usize = super::adjacency_graphs::KEYPAD.len();
}

fn calc_average_degree(graph: &HashMap<char, Vec<Option<&'static str>>>) -> usize {
    let sum: usize =
        graph.values().map(|neighbors| neighbors.iter().filter(|n| n.is_some()).count()).sum();
    sum / graph.len()
}

struct RepeatEstimator {}

impl Estimator for RepeatEstimator {
    fn estimate(&self, m: &mut Match) -> u64 {
        m.base_guesses.unwrap() * m.repeat_count.unwrap() as u64
    }
}

struct SequenceEstimator {}

impl Estimator for SequenceEstimator {
    fn estimate(&self, m: &mut Match) -> u64 {
        let first_chr = m.token.chars().next().unwrap();
        // lower guesses for obvious starting points
        let mut base_guesses = if ['a', 'A', 'z', 'Z', '0', '1', '9'].contains(&first_chr) {
            4
        } else if first_chr.is_digit(10) {
            10
        } else {
            // could give a higher base for uppercase,
            // assigning 26 to both upper and lower sequences is more conservative.
            26
        };
        if !m.ascending.unwrap_or(false) {
            // need to try a descending sequence in addition to every ascending sequence ->
            // 2x guesses
            base_guesses *= 2;
        }
        base_guesses * m.token.len() as u64
    }
}

struct RegexEstimator {}

impl Estimator for RegexEstimator {
    fn estimate(&self, m: &mut Match) -> u64 {
        if CHAR_CLASS_BASES.keys().any(|x| x == &m.regex_name.unwrap()) {
            CHAR_CLASS_BASES[m.regex_name.unwrap()].pow(m.token.len() as u32)
        } else {
            match m.regex_name {
                Some("recent_year") => {
                    let year_space = (m.regex_match.as_ref().unwrap()[0].parse::<i16>().unwrap() -
                                      REFERENCE_YEAR)
                        .abs();
                    cmp::max(year_space, MIN_YEAR_SPACE) as u64
                }
                _ => unreachable!(),
            }
        }
    }
}

lazy_static! {
    static ref CHAR_CLASS_BASES: HashMap<&'static str, u64> = {
        let mut table = HashMap::with_capacity(6);
        table.insert("alpha_lower", 26);
        table.insert("alpha_upper", 26);
        table.insert("alpha", 52);
        table.insert("alphanumeric", 62);
        table.insert("digits", 10);
        table.insert("symbols", 33);
        table
    };
}

struct DateEstimator {}

impl Estimator for DateEstimator {
    fn estimate(&self, m: &mut Match) -> u64 {
        // base guesses: (year distance from REFERENCE_YEAR) * num_days * num_years
        let year_space = cmp::max((m.year.unwrap() - REFERENCE_YEAR).abs(), MIN_YEAR_SPACE);
        let mut guesses = year_space * 365;
        // add factor of 4 for separator selection (one of ~4 choices)
        if m.separator.is_some() {
            guesses *= 4;
        }
        guesses as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::TestResult;

    #[test]
    fn test_n_ck() {
        let test_data = [(0, 0, 1),
                         (1, 0, 1),
                         (5, 0, 1),
                         (0, 1, 0),
                         (0, 5, 0),
                         (2, 1, 2),
                         (4, 2, 6),
                         (33, 7, 4272048)];
        for &(n, k, result) in &test_data {
            assert_eq!(n_ck(n, k), result);
        }
    }

    quickcheck! {
        fn test_n_ck_mul_overflow(n: usize, k: usize) -> TestResult {
            if n >= 63 {
                n_ck(n, k); // Must not panic
                TestResult::from_bool(true)
            } else {
                TestResult::discard()
            }
        }

        fn test_n_ck_mirror_identity(n: usize, k: usize) -> TestResult {
            if k > n || n >= 63 {
                return TestResult::discard();
            }
            TestResult::from_bool(n_ck(n, k) == n_ck(n, n-k))
        }

        fn test_n_ck_pascals_triangle(n: usize, k: usize) -> TestResult {
            if n == 0 || k == 0 || n >= 63 {
                return TestResult::discard();
            }
            TestResult::from_bool(n_ck(n, k) == n_ck(n-1, k-1) + n_ck(n-1, k))
        }
    }

    #[test]
    fn test_search_returns_one_bruteforce_match_given_empty_match_sequence() {
        let password = "0123456789";
        let result = most_guessable_match_sequence(password, &[], true);
        assert_eq!(result.sequence.len(), 1);
        let m0 = &result.sequence[0];
        assert_eq!(m0.pattern, "bruteforce");
        assert_eq!(m0.token, password);
        assert_eq!(m0.i, 0);
        assert_eq!(m0.j, 9);
    }

    #[test]
    fn test_search_returns_match_and_bruteforce_when_match_covers_prefix_of_password() {
        let password = "0123456789";
        let m = Match::default().i(0usize).j(5usize).guesses(Some(1)).build();

        let result = most_guessable_match_sequence(password, &[m.clone()], true);
        assert_eq!(result.sequence.len(), 2);
        assert_eq!(result.sequence[0], m);
        let m1 = &result.sequence[1];
        assert_eq!(m1.pattern, "bruteforce");
        assert_eq!(m1.i, 6);
        assert_eq!(m1.j, 9);
    }

    #[test]
    fn test_search_returns_bruteforce_and_match_when_match_covers_a_suffix() {
        let password = "0123456789";
        let m = Match::default().i(3usize).j(9usize).guesses(Some(1)).build();

        let result = most_guessable_match_sequence(password, &[m.clone()], true);
        assert_eq!(result.sequence.len(), 2);
        let m0 = &result.sequence[0];
        assert_eq!(m0.pattern, "bruteforce");
        assert_eq!(m0.i, 0);
        assert_eq!(m0.j, 2);
        assert_eq!(result.sequence[1], m);
    }

    #[test]
    fn test_search_returns_bruteforce_and_match_when_match_covers_an_infix() {
        let password = "0123456789";
        let m = Match::default().i(1usize).j(8usize).guesses(Some(1)).build();

        let result = most_guessable_match_sequence(password, &[m.clone()], true);
        assert_eq!(result.sequence.len(), 3);
        assert_eq!(result.sequence[1], m);
        let m0 = &result.sequence[0];
        let m2 = &result.sequence[2];
        assert_eq!(m0.pattern, "bruteforce");
        assert_eq!(m0.i, 0);
        assert_eq!(m0.j, 0);
        assert_eq!(m2.pattern, "bruteforce");
        assert_eq!(m2.i, 9);
        assert_eq!(m2.j, 9);
    }

    #[test]
    fn test_search_chooses_lower_guesses_match_given_two_matches_of_same_span() {
        let password = "0123456789";
        let mut m0 = Match::default().i(0usize).j(9usize).guesses(Some(1)).build();
        let m1 = Match::default().i(0usize).j(9usize).guesses(Some(2)).build();

        let result = most_guessable_match_sequence(password, &[m0.clone(), m1.clone()], true);
        assert_eq!(result.sequence.len(), 1);
        assert_eq!(result.sequence[0], m0);
        // make sure ordering doesn't matter
        m0.guesses = Some(3);
        let result = most_guessable_match_sequence(password, &[m0.clone(), m1.clone()], true);
        assert_eq!(result.sequence.len(), 1);
        assert_eq!(result.sequence[0], m1);
    }

    #[test]
    fn test_search_when_m0_covers_m1_and_m2_choose_m0_when_m0_lt_m1_t_m2_t_fact_2() {
        let password = "0123456789";
        let m0 = Match::default().i(0usize).j(9usize).guesses(Some(3)).build();
        let m1 = Match::default().i(0usize).j(3usize).guesses(Some(2)).build();
        let m2 = Match::default().i(4usize).j(9usize).guesses(Some(1)).build();

        let result =
            most_guessable_match_sequence(password, &[m0.clone(), m1.clone(), m2.clone()], true);
        assert_eq!(result.guesses, 3);
        assert_eq!(result.sequence, vec![m0]);
    }

    #[test]
    fn test_search_when_m0_covers_m1_and_m2_choose_m1_m2_when_m0_gt_m1_t_m2_t_fact_2() {
        let password = "0123456789";
        let m0 = Match::default().i(0usize).j(9usize).guesses(Some(5)).build();
        let m1 = Match::default().i(0usize).j(3usize).guesses(Some(2)).build();
        let m2 = Match::default().i(4usize).j(9usize).guesses(Some(1)).build();

        let result =
            most_guessable_match_sequence(password, &[m0.clone(), m1.clone(), m2.clone()], true);
        assert_eq!(result.guesses, 4);
        assert_eq!(result.sequence, vec![m1, m2]);
    }
}

/// Example 4: Move 2 enums, the way production contracts version their data.
///
/// Rock, paper, scissors against the house. Three kinds of enum:
///
/// - `Played` is a versioned event. A player's first game emits `V1`; later games emit
///   `V2`, which adds the player's streak. Its log table gets a column per field of
///   either variant, `streak` is null for `V1` rows, and `_variant` says which.
/// - `Record` is a versioned resource. It's created as `V1` and upgraded to `V2` in
///   place on the second game, so its mirror row changes variant.
/// - `Hand` and `Outcome` are plain labels, stored as values inside the others.
///
/// The house's hand comes from the transaction hash: unpredictable enough for a demo,
/// never for money. Real games use `aptos_framework::randomness`.
module arena::arena {
    use std::signer;
    use std::vector;
    use aptos_framework::event;
    use aptos_framework::transaction_context;

    const E_HAND: u64 = 1;

    enum Hand has copy, drop, store {
        Rock,
        Paper,
        Scissors,
    }

    enum Outcome has copy, drop, store {
        Win,
        Lose,
        Draw,
    }

    /// A player's record, at their address.
    enum Record has key {
        V1 { games: u64, wins: u64 },
        V2 { games: u64, wins: u64, streak: u64, best_streak: u64 },
    }

    #[event]
    enum Played has drop, store {
        V1 { player: address, hand: Hand, house: Hand, outcome: Outcome },
        V2 { player: address, hand: Hand, house: Hand, outcome: Outcome, streak: u64 },
    }

    /// Play `hand`: 0 for rock, 1 for paper, 2 for scissors.
    public entry fun play(player: &signer, hand: u8) acquires Record {
        let hash = transaction_context::get_transaction_hash();
        let house = *vector::borrow(&hash, 0) % 3;
        settle(player, hand, house);
    }

    fun settle(player: &signer, hand: u8, house: u8) acquires Record {
        assert!(hand < 3, E_HAND);
        let addr = signer::address_of(player);
        // Each hand beats the one before it, going around.
        let outcome = if (hand == house) {
            Outcome::Draw
        } else if ((hand + 3 - house) % 3 == 1) {
            Outcome::Win
        } else {
            Outcome::Lose
        };
        let won = outcome == Outcome::Win;

        if (!exists<Record>(addr)) {
            move_to(player, Record::V1 { games: 1, wins: if (won) 1 else 0 });
            event::emit(Played::V1 { player: addr, hand: to_hand(hand), house: to_hand(house), outcome });
            return
        };

        // Take the record out and put a V2 back: a V1 record becomes V2 here, the same
        // resource in a newer shape. The chain sees one write.
        let (games, wins, streak, best) = match (move_from<Record>(addr)) {
            Record::V1 { games, wins } => (games, wins, 0, 0),
            Record::V2 { games, wins, streak, best_streak } => (games, wins, streak, best_streak),
        };
        let streak = if (won) streak + 1 else 0;
        let best = if (streak > best) streak else best;
        move_to(player, Record::V2 {
            games: games + 1,
            wins: if (won) wins + 1 else wins,
            streak,
            best_streak: best,
        });
        event::emit(Played::V2 {
            player: addr,
            hand: to_hand(hand),
            house: to_hand(house),
            outcome,
            streak,
        });
    }

    fun to_hand(n: u8): Hand {
        if (n == 0) Hand::Rock else if (n == 1) Hand::Paper else Hand::Scissors
    }

    #[view]
    /// A player's games and wins.
    public fun record(player: address): (u64, u64) acquires Record {
        if (!exists<Record>(player)) return (0, 0);
        match (borrow_global<Record>(player)) {
            Record::V1 { games, wins } => (*games, *wins),
            Record::V2 { games, wins, .. } => (*games, *wins),
        }
    }

    #[test(alice = @0xa11ce)]
    fun records_upgrade_and_streaks_count(alice: &signer) acquires Record {
        // Paper beats rock: a win, as V1.
        settle(alice, 1, 0);
        let (games, wins) = record(@0xa11ce);
        assert!(games == 1 && wins == 1, 0);
        // Scissors beats paper: the record becomes V2. V1 didn't track streaks, so the
        // streak starts here, at 1.
        settle(alice, 2, 1);
        // Rock loses to paper: the streak ends.
        settle(alice, 0, 1);
        let (games, wins) = record(@0xa11ce);
        assert!(games == 3 && wins == 2, 1);
        match (borrow_global<Record>(@0xa11ce)) {
            Record::V2 { streak, best_streak, .. } => assert!(*streak == 0 && *best_streak == 1, 2),
            Record::V1 { .. } => abort 3,
        }
    }
}

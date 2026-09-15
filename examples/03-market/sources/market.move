/// Example 3: a marketplace, where state lives in tables and the interesting numbers
/// are ones the contract never stores.
///
/// Sellers list items for a price in the market's own credits (claim some for free, so
/// trying it costs nothing but gas), buyers buy them, sellers cancel. Open listings are
/// items of a `SmartTable`, which Aptos stores in hashed buckets, and balances are
/// items of a `Table`.
///
/// The contract keeps no totals per seller or buyer. `nineveh.yaml` next to this file
/// builds them anyway with `reduce` rules folded over the events: listings, sales and
/// revenue per seller, purchases and spend per buyer.
module market::market {
    use std::signer;
    use std::string::{Self, String};
    use aptos_std::smart_table::{Self, SmartTable};
    use aptos_std::table::{Self, Table};
    use aptos_framework::event;
    use aptos_framework::timestamp;

    /// The market's cut of each sale, in basis points: 2.5%.
    const FEE_BPS: u64 = 250;
    /// The most credits one claim gives.
    const MAX_CLAIM: u64 = 10_000;

    const E_AMOUNT: u64 = 1;
    const E_ITEM: u64 = 2;
    const E_NO_LISTING: u64 = 3;
    const E_OWN_LISTING: u64 = 4;
    const E_NOT_SELLER: u64 = 5;
    const E_BALANCE: u64 = 6;

    /// The market, at the contract's address.
    struct Market has key {
        next_id: u64,
        fees_collected: u64,
        listings: SmartTable<u64, Listing>,
        credits: Table<address, u64>,
    }

    struct Listing has store, drop, copy {
        seller: address,
        item: String,
        price: u64,
        listed_at: u64,
    }

    #[event]
    struct CreditsClaimed has drop, store {
        account: address,
        amount: u64,
        balance: u64,
    }

    #[event]
    struct Listed has drop, store {
        id: u64,
        seller: address,
        item: String,
        price: u64,
    }

    #[event]
    struct Sold has drop, store {
        id: u64,
        seller: address,
        buyer: address,
        item: String,
        price: u64,
        fee: u64,
    }

    #[event]
    struct Cancelled has drop, store {
        id: u64,
        seller: address,
    }

    fun init_module(publisher: &signer) {
        move_to(publisher, Market {
            next_id: 0,
            fees_collected: 0,
            listings: smart_table::new(),
            credits: table::new(),
        });
    }

    /// Credits to trade with, free.
    public entry fun claim_credits(account: &signer, amount: u64) acquires Market {
        assert!(amount > 0 && amount <= MAX_CLAIM, E_AMOUNT);
        let market = borrow_global_mut<Market>(@market);
        let addr = signer::address_of(account);
        let balance = table::borrow_mut_with_default(&mut market.credits, addr, 0);
        *balance = *balance + amount;
        event::emit(CreditsClaimed { account: addr, amount, balance: *balance });
    }

    /// Offer an item for `price` credits.
    public entry fun list(seller: &signer, item: String, price: u64) acquires Market {
        assert!(price > 0, E_AMOUNT);
        let length = string::length(&item);
        assert!(length > 0 && length <= 64, E_ITEM);
        let market = borrow_global_mut<Market>(@market);
        let id = market.next_id;
        let addr = signer::address_of(seller);
        smart_table::add(&mut market.listings, id, Listing {
            seller: addr,
            item,
            price,
            listed_at: timestamp::now_seconds(),
        });
        market.next_id = id + 1;
        event::emit(Listed { id, seller: addr, item, price });
    }

    /// Buy a listing: its price moves from the buyer to the seller, less the fee.
    public entry fun buy(buyer: &signer, id: u64) acquires Market {
        let market = borrow_global_mut<Market>(@market);
        assert!(smart_table::contains(&market.listings, id), E_NO_LISTING);
        let listing = smart_table::remove(&mut market.listings, id);
        let addr = signer::address_of(buyer);
        assert!(listing.seller != addr, E_OWN_LISTING);

        let balance = table::borrow_mut_with_default(&mut market.credits, addr, 0);
        assert!(*balance >= listing.price, E_BALANCE);
        *balance = *balance - listing.price;

        let fee = listing.price * FEE_BPS / 10_000;
        let proceeds = table::borrow_mut_with_default(&mut market.credits, listing.seller, 0);
        *proceeds = *proceeds + listing.price - fee;
        market.fees_collected = market.fees_collected + fee;

        event::emit(Sold {
            id,
            seller: listing.seller,
            buyer: addr,
            item: listing.item,
            price: listing.price,
            fee,
        });
    }

    /// Take your own listing down.
    public entry fun cancel(seller: &signer, id: u64) acquires Market {
        let market = borrow_global_mut<Market>(@market);
        assert!(smart_table::contains(&market.listings, id), E_NO_LISTING);
        let addr = signer::address_of(seller);
        assert!(smart_table::borrow(&market.listings, id).seller == addr, E_NOT_SELLER);
        smart_table::remove(&mut market.listings, id);
        event::emit(Cancelled { id, seller: addr });
    }

    #[view]
    public fun balance(account: address): u64 acquires Market {
        *table::borrow_with_default(&borrow_global<Market>(@market).credits, account, &0)
    }

    #[view]
    /// The id the next listing gets.
    public fun next_id(): u64 acquires Market {
        borrow_global<Market>(@market).next_id
    }

    #[view]
    public fun open_listings(): u64 acquires Market {
        smart_table::length(&borrow_global<Market>(@market).listings)
    }

    #[test(framework = @aptos_framework, publisher = @market, alice = @0xa11ce, bob = @0xb0b)]
    fun lists_sells_and_cancels(
        framework: &signer,
        publisher: &signer,
        alice: &signer,
        bob: &signer,
    ) acquires Market {
        timestamp::set_time_has_started_for_testing(framework);
        init_module(publisher);
        claim_credits(bob, 1_000);
        list(alice, string::utf8(b"lamp"), 400);
        list(alice, string::utf8(b"rug"), 100);
        buy(bob, 0);
        assert!(balance(@0xb0b) == 600, 0);
        assert!(balance(@0xa11ce) == 390, 1);
        cancel(alice, 1);
        assert!(open_listings() == 0, 2);
    }

    #[test(framework = @aptos_framework, publisher = @market, alice = @0xa11ce, bob = @0xb0b)]
    #[expected_failure(abort_code = E_BALANCE)]
    fun buyers_need_the_credits(
        framework: &signer,
        publisher: &signer,
        alice: &signer,
        bob: &signer,
    ) acquires Market {
        timestamp::set_time_has_started_for_testing(framework);
        init_module(publisher);
        list(alice, string::utf8(b"lamp"), 400);
        buy(bob, 0);
    }
}

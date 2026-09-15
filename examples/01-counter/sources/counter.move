/// Example 1: the smallest thing Nineveh can follow.
///
/// Everyone gets their own `Counter`, stored at their address. Each increment writes
/// that resource and emits an `Incremented` event.
///
/// In Studio, tick both:
/// - `Incremented` becomes a log table: one row per click, in order.
/// - `Counter` becomes a mirror table: each account's current value.
module counter::counter {
    use std::signer;
    use aptos_framework::event;

    /// One per account.
    struct Counter has key {
        value: u64,
    }

    #[event]
    /// Emitted on every increment, with the value it reached.
    struct Incremented has drop, store {
        account: address,
        value: u64,
    }

    #[event]
    /// Emitted when someone starts over.
    struct Reset has drop, store {
        account: address,
        from: u64,
    }

    /// Add one to the caller's counter, creating it the first time.
    public entry fun increment(account: &signer) acquires Counter {
        let addr = signer::address_of(account);
        if (!exists<Counter>(addr)) {
            move_to(account, Counter { value: 0 });
        };
        let counter = borrow_global_mut<Counter>(addr);
        counter.value = counter.value + 1;
        event::emit(Incremented { account: addr, value: counter.value });
    }

    /// Set the caller's counter back to zero.
    public entry fun reset(account: &signer) acquires Counter {
        let addr = signer::address_of(account);
        if (!exists<Counter>(addr)) return;
        let counter = borrow_global_mut<Counter>(addr);
        event::emit(Reset { account: addr, from: counter.value });
        counter.value = 0;
    }

    #[view]
    public fun value(account: address): u64 acquires Counter {
        if (exists<Counter>(account)) borrow_global<Counter>(account).value else 0
    }

    #[test(alice = @0xa11ce)]
    fun counts_and_resets(alice: &signer) acquires Counter {
        let addr = signer::address_of(alice);
        assert!(value(addr) == 0, 0);
        increment(alice);
        increment(alice);
        assert!(value(addr) == 2, 1);
        reset(alice);
        assert!(value(addr) == 0, 2);
    }
}

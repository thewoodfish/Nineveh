/// Example 2: text, optional fields, and a table whose items come and go.
///
/// One `Guestbook` lives at the contract's address. Its entries are items of a
/// `Table<u64, Entry>`: signing adds one, erasing deletes it. Table items never show up
/// as resource writes, so only a `table` source sees them.
///
/// In Studio, tick `Guestbook.entries` (the table) along with the events, and choose
/// "All of its history": the table is created when the contract is published, and
/// Nineveh learns which table it is from that write.
module guestbook::guestbook {
    use std::option::{Self, Option};
    use std::signer;
    use std::string::{Self, String};
    use aptos_std::table::{Self, Table};
    use aptos_framework::event;
    use aptos_framework::timestamp;

    /// A message longer than this is refused.
    const MAX_MESSAGE: u64 = 280;

    /// The message is empty or too long.
    const E_MESSAGE: u64 = 1;
    /// There's no entry with that id.
    const E_NO_ENTRY: u64 = 2;
    /// Only an entry's author can erase it.
    const E_NOT_AUTHOR: u64 = 3;

    /// The book, at the contract's address.
    struct Guestbook has key {
        entries: Table<u64, Entry>,
        /// Entries ever signed, so ids are never reused.
        signed: u64,
        /// Entries currently in the book.
        open: u64,
    }

    struct Entry has store, drop, copy {
        author: address,
        message: String,
        /// The entry this answers, if any.
        reply_to: Option<u64>,
        signed_at: u64,
    }

    #[event]
    struct Signed has drop, store {
        id: u64,
        author: address,
        message: String,
        reply_to: Option<u64>,
    }

    #[event]
    struct Erased has drop, store {
        id: u64,
        author: address,
    }

    fun init_module(publisher: &signer) {
        move_to(publisher, Guestbook { entries: table::new(), signed: 0, open: 0 });
    }

    /// Write a message in the book.
    public entry fun sign(author: &signer, message: String) acquires Guestbook {
        add(author, message, option::none());
    }

    /// Answer an entry that's still in the book.
    public entry fun reply(author: &signer, to: u64, message: String) acquires Guestbook {
        assert!(table::contains(&borrow_global<Guestbook>(@guestbook).entries, to), E_NO_ENTRY);
        add(author, message, option::some(to));
    }

    /// Take one of your own entries out of the book.
    public entry fun erase(author: &signer, id: u64) acquires Guestbook {
        let book = borrow_global_mut<Guestbook>(@guestbook);
        assert!(table::contains(&book.entries, id), E_NO_ENTRY);
        let entry = table::remove(&mut book.entries, id);
        let addr = signer::address_of(author);
        assert!(entry.author == addr, E_NOT_AUTHOR);
        book.open = book.open - 1;
        event::emit(Erased { id, author: addr });
    }

    fun add(author: &signer, message: String, reply_to: Option<u64>) acquires Guestbook {
        let length = string::length(&message);
        assert!(length > 0 && length <= MAX_MESSAGE, E_MESSAGE);
        let book = borrow_global_mut<Guestbook>(@guestbook);
        let id = book.signed;
        let addr = signer::address_of(author);
        table::add(&mut book.entries, id, Entry {
            author: addr,
            message,
            reply_to,
            signed_at: timestamp::now_seconds(),
        });
        book.signed = id + 1;
        book.open = book.open + 1;
        event::emit(Signed { id, author: addr, message, reply_to });
    }

    #[view]
    /// The id the next entry gets.
    public fun next_id(): u64 acquires Guestbook {
        borrow_global<Guestbook>(@guestbook).signed
    }

    #[view]
    public fun open_entries(): u64 acquires Guestbook {
        borrow_global<Guestbook>(@guestbook).open
    }

    #[test(framework = @aptos_framework, publisher = @guestbook, alice = @0xa11ce, bob = @0xb0b)]
    fun signs_replies_and_erases(
        framework: &signer,
        publisher: &signer,
        alice: &signer,
        bob: &signer,
    ) acquires Guestbook {
        timestamp::set_time_has_started_for_testing(framework);
        init_module(publisher);
        sign(alice, string::utf8(b"gm"));
        reply(bob, 0, string::utf8(b"gm to you"));
        assert!(open_entries() == 2, 0);
        erase(alice, 0);
        assert!(open_entries() == 1, 1);
    }

    #[test(framework = @aptos_framework, publisher = @guestbook, alice = @0xa11ce, bob = @0xb0b)]
    #[expected_failure(abort_code = E_NOT_AUTHOR)]
    fun only_authors_erase(
        framework: &signer,
        publisher: &signer,
        alice: &signer,
        bob: &signer,
    ) acquires Guestbook {
        timestamp::set_time_has_started_for_testing(framework);
        init_module(publisher);
        sign(alice, string::utf8(b"mine"));
        erase(bob, 0);
    }
}

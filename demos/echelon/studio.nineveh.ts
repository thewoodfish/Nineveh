// The same fold as echelon.nineveh.ts, against the source names Studio derives from
// the struct names (`BorrowEvent` -> `borrow_event`) rather than the ones the
// hand-written nineveh.yaml chose. Paste this one into Studio.
//
// Three events name the borrower three different ways — account_addr on a borrow,
// borrower_addr on a repayment and on a liquidation — which is why the wizard won't
// offer a shared key. The handler maps it per source, which is what row() is for.
export const borrowers = table({
  key:     { account: address },
  columns: {
    borrows:        u64.default(0),
    repays:         u64.default(0),
    liquidations:   u64.default(0),
    borrowed_total: u128.default(0),
  },
})

on(borrow_event, (b) => {
  const r = borrowers.row(b.account_addr)
  r.borrows        += 1
  r.borrowed_total += u128(b.amount)
})

on(repay_event, (p) => {
  borrowers.row(p.borrower_addr).repays += 1
})

on(liquidate_event, (l) => {
  borrowers.row(l.borrower_addr).liquidations += 1
})

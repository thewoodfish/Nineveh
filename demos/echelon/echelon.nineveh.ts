// What each account has done, which no single record carries.
//
// A borrow, a repayment and a liquidation are three different events naming the
// account three different ways. Folded here they are one row per borrower.
export const borrowers = table({
  key:     { account: address },
  columns: {
    borrows:        u64.default(0),
    repays:         u64.default(0),
    liquidations:   u64.default(0),
    borrowed_total: u128.default(0),
  },
})

on(borrowed, (b) => {
  const r = borrowers.row(b.account_addr)
  r.borrows        += 1
  r.borrowed_total += u128(b.amount)
})

on(repaid, (p) => {
  borrowers.row(p.borrower_addr).repays += 1
})

on(liquidated, (l) => {
  borrowers.row(l.borrower_addr).liquidations += 1
})

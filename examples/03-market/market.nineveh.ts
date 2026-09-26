// The numbers the market never stores.
//
// `nineveh.yaml` beside this file says what to follow, and keeps the tables that are
// copies of what the contract holds: the open listings, everyone's credits, every sale
// in order. This file is the other half. A marketplace's most obvious questions are
// about people rather than listings, and the contract answers none of them: it has no
// idea who its best seller is, because keeping a running total would cost gas on every
// trade.
//
// Folding the events it already emits costs nothing and answers them all.

export const sellers = table({
  key:     { seller: address },
  columns: {
    listed:    u64.default(0),
    sold:      u64.default(0),
    cancelled: u64.default(0),
    revenue:   u128.default(0),
  },
})

export const buyers = table({
  key:     { buyer: address },
  columns: {
    bought: u64.default(0),
    spent:  u128.default(0),
  },
})

on(listed, (l) => {
  sellers.row(l.seller).listed += 1
})

// A sale is the only record that moves credits, so it writes both tables. The seller
// keeps the price less the market's cut; the buyer is out the whole price.
on(sold, (s) => {
  const seller = sellers.row(s.seller)
  seller.sold    += 1
  seller.revenue += u128(s.price - s.fee)

  const buyer = buyers.row(s.buyer)
  buyer.bought += 1
  buyer.spent  += u128(s.price)
})

on(cancelled, (c) => {
  sellers.row(c.seller).cancelled += 1
})

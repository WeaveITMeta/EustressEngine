# Manufacturer registry

One TOML file per manufacturer. `ManufacturingPlugin` reads this directory at
startup into `ManufacturingProgramRegistry`, which backs the RFQ Builder's vendor
picker and resolves vendor names on every purchase order.

## What is in here

The Voltec V-Cell supply base: eight vendors covering the cell's bill of
materials from scandium oxide through pack integration. They exist to make the
Voltec > Supply Chain demo Space work end to end, so the RFQ Builder opens with a
real vendor list and the Purchase Order Tracker opens with real orders against
those vendors.

These are scenario records, not real companies. Every quantity, price, lead time,
audit score, and certification is authored for the demo. Replace them with real
vendors before any of this informs a real purchasing decision.

## The scenario

`voltec-scandium-refinery` is the deliberate constraint. It is the only qualified
source of 4N scandium oxide for the Sc-NASICON solid electrolyte, at a 126 day
lead time and 420 kg per month. `voltec-nasicon-sintering` cannot run without it,
and cell assembly cannot run without the membrane, so a single vendor gates every
V-Cell shipped. That is what the demo Space visualises and what the seeded orders
are responding to: `PO00005` is an open RFQ qualifying a second source.

## This registry is global

Manufacturers are shared reference data read from a path relative to the working
directory, so every Space sees the same vendor list. Orders are not: they live in
the Space at `<space>/Manufacturing/PurchaseOrders/`, which is what lets two
Spaces hold different orders against the same manufacturer.

If you open an unrelated Space, the RFQ Builder will still offer these Voltec
vendors. That is the current design, not a bug, and it is the thing to revisit if
vendor lists ever need to be per-project.

## Status gates allocation

Only `status = "approved"` manufacturers appear in the vendor picker.
`pending_audit`, `suspended`, and `blacklisted` vendors cannot be allocated work
elsewhere in the manufacturing module, so offering one here would produce an
order the program would then refuse to honour. An `audit_score` of 80 or above is
the documented bar for `approved`.

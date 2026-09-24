;redcode-94
;name Seed Sentry
;author board-corewar seeds
;strategy Sits still and keeps decrementing a cell behind it: an imp gate.
;assert CORESIZE == 8000
gate    JMP.B   gate, <-20

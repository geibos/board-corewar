;redcode-94
;name Seed Stunner
;author board-corewar seeds
;strategy Bombs with SPL 0: a hit enemy process starts splitting in place.
;assert CORESIZE == 8000
loop    ADD.AB  #5, target
        MOV.I   stun, @target
        JMP     loop
stun    SPL.B   #0, #0
target  DAT.F   #0, #0

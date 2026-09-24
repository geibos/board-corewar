;redcode-94
;name Seed Hammer
;author board-corewar seeds
;strategy A bomber: drops a DAT every fourth cell and never moves.
;assert CORESIZE == 8000
loop    ADD.AB  #4, bomb
        MOV.I   bomb, @bomb
        JMP     loop
bomb    DAT.F   #0, #0

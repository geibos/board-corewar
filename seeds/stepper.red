;redcode-94
;name Seed Stepper
;author board-corewar seeds
;strategy A bomber with a long step, so its bombs spread over the core fast.
;assert CORESIZE == 8000
step    EQU     2365
loop    ADD.AB  #step, bomb
        MOV.I   bomb, @bomb
        JMP     loop
bomb    DAT.F   #0, #0

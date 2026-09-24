;redcode-94
;name Seed Sweeper
;author board-corewar seeds
;strategy Clears the core ahead of it with DATs, stops just short of itself
;strategy and waits there.
;assert CORESIZE == 8000
ptr     DAT.F   #0, #4
loop    MOV.I   bomb, >ptr
        DJN.B   loop, #7994
        JMP.B   $0
bomb    DAT.F   #0, #0
        END     loop

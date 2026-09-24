;redcode-94
;name Seed Mice
;author board-corewar seeds, after Mice by Chip Wendell (1986)
;strategy A replicator: copies itself 653 cells on and starts the copy.
;assert CORESIZE == 8000
ptr     DAT.F   #0, #0
start   MOV.AB  #12, ptr
loop    MOV.I   @ptr, <dest
        DJN.B   loop, ptr
        SPL.B   @dest
        ADD.AB  #653, dest
        JMZ.B   start, ptr
dest    DAT.F   #0, #833
        END     start

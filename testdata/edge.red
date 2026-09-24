;redcode-94
;name edge
;assert 1
x  EQU 3*(2+1)
a  dat 5
   jmp 0
   spl a
   mov <a, }x
   add #-1, @CORESIZE-1
   dat #1, 2
   seq a, a+x
   nop 0
   djn.f $a, {a
   mul.x > 1, * 2
   end

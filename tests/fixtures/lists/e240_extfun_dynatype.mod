// inventory: lists_e240_extfun_dynatype
// `foo` is an external-function name, which is in their table. Our W160 also
// fires here (the named companion is absent); they never emit it.
external_function(name=foo);
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

dynatype('f.m') foo;

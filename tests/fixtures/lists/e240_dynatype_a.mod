// inventory: lists_e240_dynatype_a
// `dynatype` takes endogenous and exogenous; `a` is a parameter.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

dynatype('f.m') a;

// inventory: mom_e388_dup_pair
// One `matched_irfs` block listing the same endogenous/shock pair twice. The
// periods differ; the pair is what 7.1 keys on.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

matched_irfs;
var y; varexo e; periods 1; values 1;
var y; varexo e; periods 2; values 2;
end;

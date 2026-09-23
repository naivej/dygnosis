// inventory: d_carry_e430_range_missing
var y;
varexo e;
parameters a;
a=0.5;
model;
y=a*y(-1)+e;
end;
a.subsamples(s=2000Q1:2000Q2);
a.t.options(init=0);

// inventory: d_carry_e427_range_twice
var y;
varexo e;
parameters a;
a=0.5;
model;
y=a*y(-1)+e;
end;
a.subsamples(s=2000Q1:2000Q2,s=2001Q1:2001Q2);

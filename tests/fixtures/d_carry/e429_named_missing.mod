// inventory: d_carry_e429_named_missing
var y;
varexo e;
parameters a;
a=0.5;
model;
y=a*y(-1)+e;
end;
a.s.prior(shape=normal,mean=0,stdev=1);

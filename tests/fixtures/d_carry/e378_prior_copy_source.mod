// inventory: d_carry_e378_prior_copy_source
var y;
varexo e;
parameters a;
a=.5;
model;
y=a*y(-1)+e;
end;
a.s.prior=y.s.prior;

// inventory: d_carry_e001_subsample_date
var y;
varexo e;
parameters a;
a=0.5;
model;
y=a*y(-1)+e;
end;
a.subsamples(s=2000Q1:4);

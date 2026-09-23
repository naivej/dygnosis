// inventory: d_carry_e431_writer_copy
var y;
varexo e;
varexo_det d;
parameters a;
a=0.5;
model;
y=a*y(-1)+e;
end;
a.subsamples(s=2000Q1:2000Q2);
d.subsamples=a.subsamples;

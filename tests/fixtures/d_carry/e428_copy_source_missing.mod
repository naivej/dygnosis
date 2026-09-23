// inventory: d_carry_e428_copy_source_missing
var y;
varexo e;
parameters a b;
a=0.5;
b=0.2;
model;
y=a*y(-1)+b+e;
end;
b.subsamples=a.subsamples;

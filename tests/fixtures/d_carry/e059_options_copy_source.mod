// inventory: d_carry_e059_options_copy_source
var y;
varexo e u;
parameters a;
a=.5;
model;
y=a*y(-1)+e+u;
end;
corr(e,u).s.options=corr(e,a).s.options;

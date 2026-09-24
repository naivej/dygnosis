// inventory: d_pac_rewrite_s014_no_target
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] y = b*y(-1)+e+pac_target_nonstationary(nope);
  [name='Z'] z = z(-1)+e2;
end;

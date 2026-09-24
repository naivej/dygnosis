// inventory: d_pac_w206_deterministic_param
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] y = b*y(-1)+e;
  [name='Z'] z = z(-1)+e2;
end;
parameters p; deterministic_trends; p(1); end;

// inventory: d_pac_e445_epilogue_target
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] y = b*y(-1)+e;
  [name='Z'] z = z(-1)+e2;
end;
epilogue; q = pac_target_nonstationary(q); end;

// inventory: e294_epilogue_outside
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
epilogue;
foo = y;
end;
var y2;
model;
y2 = foo;
end;

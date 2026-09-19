// inventory: e030_epilogue_and_var
var foo;
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
epilogue;
foo = 1;
end;

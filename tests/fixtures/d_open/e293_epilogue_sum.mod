// inventory: e293_epilogue_sum
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
epilogue;
foo = SUM(y);
end;

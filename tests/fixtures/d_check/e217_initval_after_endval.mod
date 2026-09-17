// inventory: e217_initval_after_endval
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
endval;
y = 0;
end;
initval;
y = 0;
end;

// inventory: e292_epilogue_steady_state
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
epilogue;
foo = STEADY_STATE(y);
end;

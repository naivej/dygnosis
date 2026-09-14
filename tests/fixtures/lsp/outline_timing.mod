// inventory: lsp_outline_timing
var y k w a u;
varexo e;
parameters rho;
rho = 0.9;

model;
y = rho * y(-1) + e;
k = k(+1) + k(-1);
w = y;
# loc = y + 1;
[static] a = 0;
end;

initval;
y = 0;
k = 0;
w = 0;
a = 0;
u = 0;
end;

// inventory: e263_mcp_lhs_not_endo
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
[mcp = 'e > 0'] y = rho * y(-1) + e;
end;

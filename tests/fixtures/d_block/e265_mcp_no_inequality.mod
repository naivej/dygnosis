// inventory: e265_mcp_no_inequality
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
[mcp = 'y'] y = rho * y(-1) + e;
end;

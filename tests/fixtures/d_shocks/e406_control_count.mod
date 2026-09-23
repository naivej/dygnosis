var y;
varexo e u;
parameters p;
p=1;
model;
y=e+u+p;
end;
shock_paths; exogenize y; periods 1, 2; values p; endogenize e; end;

// inventory: e001_delete_matched_moments_end
var y c;
varexo e;
parameters a;
a = 0.5;
model;
y = a*y(-1) + e;
c = y;
end;
matched_moments;
y;

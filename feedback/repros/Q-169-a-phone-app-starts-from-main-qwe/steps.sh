# Reads the launchers, as a phone build cannot be made in a repro.
m=../../../mobile
grep -Eq 'app_value (main|entry)' $m/app-config.sh && echo "app.toml can name the entry: yes" || echo "app.toml can name the entry: no"
grep -n 'Quillo.start(' $m/android/app/src/main/java/org/quillolang/app/MainActivity.kt | sed 's/^[0-9]*: *//'
grep -o '<key>QuilloMain</key><string>[^<]*' $m/ios/Quillo/Info.plist

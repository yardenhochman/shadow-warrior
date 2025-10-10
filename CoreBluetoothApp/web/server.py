#!/usr/bin/env python3
"""
Simple HTTP server to serve the BLE web client over HTTPS
Required for Web Bluetooth API to work properly
"""

import http.server
import socketserver
import ssl
import os
import sys
from pathlib import Path

# Configuration
PORT = 8443
CERT_FILE = 'server.crt'
KEY_FILE = 'server.key'

def create_self_signed_cert():
    """Create a self-signed certificate for HTTPS"""
    try:
        from cryptography import x509
        from cryptography.x509.oid import NameOID
        from cryptography.hazmat.primitives import hashes, serialization
        from cryptography.hazmat.primitives.asymmetric import rsa
        from datetime import datetime, timedelta
        import ipaddress
        
        # Generate private key
        private_key = rsa.generate_private_key(
            public_exponent=65537,
            key_size=2048,
        )
        
        # Create certificate
        subject = issuer = x509.Name([
            x509.NameAttribute(NameOID.COUNTRY_NAME, "US"),
            x509.NameAttribute(NameOID.STATE_OR_PROVINCE_NAME, "CA"),
            x509.NameAttribute(NameOID.LOCALITY_NAME, "San Francisco"),
            x509.NameAttribute(NameOID.ORGANIZATION_NAME, "Shadow Warrior"),
            x509.NameAttribute(NameOID.COMMON_NAME, "localhost"),
        ])
        
        cert = x509.CertificateBuilder().subject_name(
            subject
        ).issuer_name(
            issuer
        ).public_key(
            private_key.public_key()
        ).serial_number(
            x509.random_serial_number()
        ).not_valid_before(
            datetime.utcnow()
        ).not_valid_after(
            datetime.utcnow() + timedelta(days=365)
        ).add_extension(
            x509.SubjectAlternativeName([
                x509.DNSName("localhost"),
                x509.IPAddress(ipaddress.IPv4Address("127.0.0.1")),
            ]),
            critical=False,
        ).sign(private_key, hashes.SHA256())
        
        # Write certificate and key to files
        with open(CERT_FILE, "wb") as f:
            f.write(cert.public_bytes(serialization.Encoding.PEM))
        
        with open(KEY_FILE, "wb") as f:
            f.write(private_key.private_bytes(
                encoding=serialization.Encoding.PEM,
                format=serialization.PrivateFormat.PKCS8,
                encryption_algorithm=serialization.NoEncryption()
            ))
        
        print(f"✅ Created self-signed certificate: {CERT_FILE}")
        print(f"✅ Created private key: {KEY_FILE}")
        return True
        
    except ImportError:
        print("❌ cryptography library not found. Installing...")
        os.system("pip install cryptography")
        return create_self_signed_cert()
    except Exception as e:
        print(f"❌ Error creating certificate: {e}")
        return False

def main():
    # Change to the web directory
    web_dir = Path(__file__).parent
    os.chdir(web_dir)
    
    # Check if certificate files exist
    if not os.path.exists(CERT_FILE) or not os.path.exists(KEY_FILE):
        print("🔐 Creating self-signed certificate for HTTPS...")
        if not create_self_signed_cert():
            print("❌ Failed to create certificate. Exiting.")
            sys.exit(1)
    
    # Create HTTPS server
    handler = http.server.SimpleHTTPRequestHandler
    
    with socketserver.TCPServer(("", PORT), handler) as httpd:
        # Wrap with SSL
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(CERT_FILE, KEY_FILE)
        httpd.socket = context.wrap_socket(httpd.socket, server_side=True)
        
        print(f"🚀 HTTPS server running on https://localhost:{PORT}")
        print(f"📁 Serving files from: {web_dir}")
        print(f"🔗 Open in browser: https://localhost:{PORT}")
        print(f"📱 For iOS Bluefy: https://[YOUR_MAC_IP]:{PORT}")
        print("\n⚠️  Note: You'll need to accept the self-signed certificate warning")
        print("   Click 'Advanced' → 'Proceed to localhost (unsafe)' in your browser")
        print("\nPress Ctrl+C to stop the server")
        
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print("\n🛑 Server stopped")

if __name__ == "__main__":
    main()

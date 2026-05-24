FROM pyd4vinci/scrapling:latest

WORKDIR /app

# Copy project files
COPY pyproject.toml .
COPY app/ app/
COPY run.py .
COPY debug_check.py .

# Install dependencies
RUN pip install --no-cache-dir fastapi "uvicorn[standard]" python-dotenv "twitchio>=2.10.0,<3.0.0" httpx curl_cffi

EXPOSE 9622

ENTRYPOINT ["python"]
CMD ["run.py"]
